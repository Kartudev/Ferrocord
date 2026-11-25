//! The `Message` enum: UI events, network events and task completions.

use iced::widget::scrollable::Viewport;

use std::path::PathBuf;

use super::emoji::{PickerTarget, Section};
use super::folders::Folders;
use super::panels::Popover;
use super::rail::RailItem;
use super::upload::UploadEvent;
use crate::net;
use crate::net::extras::{
    ChannelNotify, DiscoverCategory, DiscoverPage, InvitePreview, MuteDuration, NotifyLevel,
};
use crate::net::model::*;
use crate::net::remote_auth::RaEvent;
use crate::net::rest::UploadFile;

#[derive(Debug, Clone)]
pub enum Message {
    Ui(Ui),
    Net(net::Event),
    Done(Done),
    /// Events from the system tray service.
    Tray(crate::tray::Event),
    /// Events from the running voice connection.
    Voice(crate::net::voice::VoiceEvent),
    /// Plugins: page input, plugin-thread output, task results.
    Plugin(super::plugins::PluginMsg),
    /// In-app updater (`crate::update`).
    Update(crate::update::UpdateMsg),
}

/// User input.
#[derive(Debug, Clone)]
pub enum Ui {
    TokenInput(String),
    SubmitToken,
    RetryQr,
    /// "Log in with Discord's page": open discord.com/login in the helper.
    OpenWebLogin,
    /// Close Discord's page (kills the helper).
    CancelWebLogin,
    SelectHome,
    SelectGuild(GuildId),
    SelectChannel(ChannelId),
    ToggleCategory(ChannelId),
    /// Test/automation hook: replace the composer text.
    ComposerChanged(String),
    /// An editor action in the composer.
    ComposerAction(iced::widget::text_editor::Action),
    Send,
    Scrolled(Viewport),
    RevealSpoiler(MessageId),
    OpenUrl(String),
    // -- video player
    /// Play a video attachment or embed (downloads it first).
    PlayVideo(Box<crate::media::clip::Clip>),
    /// Stop the video and close its full-screen view.
    CloseVideo,
    VideoToggle,
    /// The seek slider moved to this position (ms).
    VideoSeekDrag(u64),
    VideoSeekRelease,
    VideoVolume(u8),
    VideoMute,
    VideoFullscreen,
    /// A link whose visible text differs from its target: ask first.
    ConfirmLink(String),
    CancelLink,
    /// Shows an image or GIF full-window.
    OpenLightbox(super::lightbox::Lightbox),
    CloseLightbox,
    /// The "X" on a message's embeds: sets `SUPPRESS_EMBEDS` on the message.
    SuppressEmbeds(MessageId),
    Logout,
    WindowFocus(bool),
    WindowOpened,
    Tick,
    /// The toast set at this generation has been shown long enough.
    ToastExpired(u64),
    // -- replies, jumps
    StartReply(MessageId),
    CancelReply,
    ToggleReplyMention,
    JumpTo {
        channel: ChannelId,
        message: MessageId,
    },
    JumpToPresent,
    // -- edit and delete
    StartEdit(MessageId),
    EditChanged(String),
    /// An editor action in the inline edit box.
    EditAction(iced::widget::text_editor::Action),
    SubmitEdit,
    CancelEdit,
    /// Up arrow in an empty composer.
    EditLast,
    DeleteClicked(MessageId),
    ConfirmDelete,
    CancelDelete,
    /// Escape: closes the topmost popup / cancels edit or reply.
    Escape,
    // -- message menus
    ContextMenu(MessageId),
    CloseMenu,
    CopyText(MessageId),
    CopyLink(MessageId),
    // -- reactions and emoji
    ToggleReaction(MessageId, Emoji),
    OpenEmojiPicker(PickerTarget),
    EmojiQuery(String),
    EmojiSection(Section),
    PickEmoji(Emoji),
    CloseEmojiPicker,
    /// "Send as image" on the Nitro lock footer of the emoji or sticker
    /// picker: uploads a scaled copy of the locked item.
    SendLockedAsImage,
    /// Closes the Nitro lock footer of the open picker.
    DismissLocked,
    /// Composer GIF button.
    OpenGifPicker,
    GifQuery(String),
    /// Sends the GIF with this Tenor URL to the open channel.
    PickGif(String),
    CloseGifPicker,
    /// Opens a page of the GIF picker's home screen (or goes back to it).
    GifPage(super::gifs::GifPage),
    /// The picker grid scrolled to this offset (px from the top).
    GifScrolled(f32),
    /// The pointer entered a picker tile (its page URL).
    GifHoverEnter(String),
    /// The pointer left a picker tile.
    GifHoverLeave(String),
    /// Saves or unsaves this GIF (the star on a picker tile).
    ToggleGifFavorite(Box<crate::net::model::Gif>),
    /// Composer sticker button.
    OpenStickerPicker,
    StickerQuery(String),
    /// Sends this sticker to the open channel.
    PickSticker(Snowflake),
    CloseStickerPicker,
    // -- attachments
    PickFiles,
    FileDropped(PathBuf),
    Paste,
    RemoveAttachment(usize),
    CancelUpload(u64),
    // -- member list, status, search
    ToggleMembers,
    MembersScrolled(Viewport),
    ToggleStatusMenu,
    SetStatus(Presence),
    SearchChanged(String),
    SubmitSearch,
    SearchPage(bool),
    CloseSearch,
    /// A notification was clicked.
    OpenChannel(ChannelId),
    // -- window and tray
    /// The window's close button (or Alt+F4).
    CloseRequested,
    ToggleWindow,
    ShowWindow,
    HideWindow,
    Quit,
    OpenSettings,
    CloseSettings,
    ToggleCloseToTray,
    ToggleStartMinimized,
    /// Tray "Mute notifications" / settings toggle.
    ToggleMuted,
    ToggleAutostart,
    /// "Reduce motion": no avatar, emoji, decoration or effect animation.
    ToggleReduceMotion,
    /// "Play GIFs automatically" in messages.
    ToggleAutoplayGifs,
    // -- profile popout and animation
    OpenProfile {
        user: UserId,
        guild: Option<GuildId>,
    },
    CloseProfile,
    /// The pointer entered a row / message with an avatar to animate.
    HoverEnter(super::anim::Hover),
    HoverLeave(super::anim::Hover),
    // -- channel header popovers
    TogglePopover(Popover),
    ClosePopover,
    SetChannelLevel(ChannelId, NotifyLevel),
    MuteChannel(ChannelId, MuteDuration),
    UnmuteChannel(ChannelId),
    Unpin(ChannelId, MessageId),
    InboxMore,
    InboxGuildOnly(bool),
    // -- add a server
    OpenAddServer,
    CloseAddServer,
    AddServerBack,
    AddServerCreate,
    AddServerJoin,
    CreateNameChanged(String),
    PickServerIcon,
    ClearServerIcon,
    SubmitCreate,
    JoinInputChanged(String),
    SubmitJoin,
    ConfirmJoin,
    // -- discover
    OpenDiscover,
    CloseDiscover,
    DiscoverInput(String),
    DiscoverSubmit,
    DiscoverCategory(Option<u32>),
    DiscoverMore,
    DiscoverJoin(GuildId),
    DiscoverDetails(Option<GuildId>),
    // -- server rail: folders and drag and drop
    RailPress(RailItem),
    RailRelease(RailItem),
    RailEnter(RailItem),
    RailExit,
    /// Any left-button release: a drag that did not end on a rail item.
    DragEnd,
    RailMenu(RailItem),
    CloseRailMenu,
    // -- context menus (users, channels, servers)
    ContextOpen(super::context::CtxTarget),
    ContextClose,
    CopyToClipboard(String),
    MentionUser(UserId),
    MarkChannelRead(ChannelId),
    MarkGuildRead(GuildId),
    MuteGuild(GuildId, bool),
    SuppressEveryone(GuildId, bool),
    ToggleFolder(u64),
    CreateFolder(GuildId),
    MoveToFolder(GuildId, u64),
    RemoveFromFolder(GuildId),
    UngroupFolder(u64),
    OpenFolderSettings(u64),
    FolderNameChanged(String),
    FolderColor(Option<u32>),
    SaveFolder,
    CloseFolderSettings,
    // -- voice channels and calls
    Voice(super::voice::VoiceUi),
    /// User Settings screen.
    Settings(super::settings::SettingsUi),
}

/// Completion of a `Task`.
#[derive(Debug, Clone)]
pub enum Done {
    /// Quit: the bounded shutdown work finished (or ran out of time).
    QuitReady,
    /// Autostart entry written or removed: the resulting state.
    AutostartChanged(Result<bool, String>),
    /// Initial autostart state read from disk.
    AutostartLoaded(bool),
    FontsLoaded,
    /// Keyring read: `Ok(true)` if a token was found (it is moved into the
    /// app through `TokenReady`, never through a cloned message).
    TokenRestored(Result<Option<crate::auth::secrets::Secret>, String>),
    /// `GET /users/@me` result for a token being validated.
    TokenValidated {
        result: Result<User, String>,
        revoked: bool,
    },
    TokenSaved(Result<(), String>),
    Qr(RaEvent),
    WebLogin(crate::auth::web_login::WebLoginEvent),
    CachedHistory {
        channel: ChannelId,
        messages: Vec<crate::net::model::Message>,
    },
    History {
        channel: ChannelId,
        older: bool,
        result: Result<Vec<crate::net::model::Message>, String>,
    },
    Sent {
        channel: ChannelId,
        nonce: String,
        result: Result<Box<crate::net::model::Message>, String>,
    },
    Image {
        key: String,
        bytes: Option<Vec<u8>>,
    },
    /// Download progress or helper event of a video playback session.
    Video {
        session: u32,
        progress: super::video::Progress,
    },
    Edited {
        channel: ChannelId,
        message: MessageId,
        result: Result<Box<crate::net::model::Message>, String>,
    },
    ActionFailed(String),
    /// `SUPPRESS_EMBEDS` was refused: put the embeds back.
    SuppressFailed {
        channel: ChannelId,
        message: MessageId,
        embeds: Vec<Embed>,
        flags: u64,
        error: String,
    },
    ReactionFailed {
        channel: ChannelId,
        message: MessageId,
        emoji: Emoji,
        change: super::state::ReactionChange,
        error: String,
    },
    /// Files chosen in the dialog or dropped (already stat-ed).
    FilesReady(Vec<Result<(UploadFile, bool), String>>),
    /// Result of a Ctrl+V clipboard probe; `pasted` are file paths that may
    /// also have been pasted as text.
    Pasted {
        files: Vec<Result<(UploadFile, bool), String>>,
        pasted: Vec<PathBuf>,
    },
    ClearHighlight(MessageId),
    /// User Settings tasks.
    Settings(Box<super::settings::SettingsDone>),
    Upload {
        id: u64,
        event: UploadEvent,
    },
    /// The scaled copy of a locked emoji or sticker ("Send as image"),
    /// written to a temporary file, or why it could not be made.
    ImageCopy {
        channel: ChannelId,
        reply: Option<crate::net::rest::ReplyTarget>,
        result: Result<UploadFile, String>,
    },
    /// A window around a message (jump), or `None` when loading failed.
    Around {
        channel: ChannelId,
        target: MessageId,
        result: Result<Vec<crate::net::model::Message>, String>,
    },
    /// Newer messages after a detached window.
    Newer {
        channel: ChannelId,
        result: Result<Vec<crate::net::model::Message>, String>,
    },
    Search {
        generation: u64,
        result: Result<SearchPage, String>,
    },
    /// The coalesced read-ack timer fired (`acks.rs`).
    FlushAcks,
    /// The GIF search debounce timer of this generation fired.
    GifSearchDue(u64),
    Gifs {
        generation: u64,
        result: Result<Vec<crate::net::model::Gif>, String>,
    },
    /// A GIF picker tile's animation, decoded off the UI thread (`None`:
    /// every source failed).
    GifAnim {
        key: String,
        anim: Option<std::sync::Arc<super::anim::Anim>>,
    },
    /// The saved GIFs were prefetched into the disk cache (files fetched).
    GifPrefetched(usize),
    /// A GIF picker still, decoded and downscaled (`None`: every source
    /// failed).
    GifThumb {
        key: String,
        image: Option<(u32, u32, Vec<u8>)>,
    },
    /// The saved GIFs after a load or a save; `rollback` restores the list
    /// from before an optimistic change that failed.
    GifFavorites {
        generation: u64,
        result: Result<Vec<crate::net::frecency::FavoriteGif>, String>,
        rollback: Option<Vec<crate::net::frecency::FavoriteGif>>,
    },
    /// Delay after a jump so the new rows are laid out before scrolling.
    ScrollToMessage(MessageId),
    StatusSaved(Result<(), String>),
    Pins {
        generation: u64,
        result: Result<Vec<crate::net::model::Message>, String>,
    },
    Mentions {
        generation: u64,
        result: Result<Vec<crate::net::model::Message>, String>,
    },
    /// Result of saving a channel's notification override; on failure the
    /// previous value is restored.
    NotifySaved {
        channel: ChannelId,
        previous: ChannelNotify,
        result: Result<(), String>,
    },
    Unpinned {
        channel: ChannelId,
        message: MessageId,
        result: Result<(), String>,
    },
    Invite {
        generation: u64,
        result: Result<InvitePreview, String>,
    },
    Joined {
        generation: u64,
        result: Result<(GuildId, String), String>,
    },
    Created(Result<(GuildId, String), String>),
    /// Server icon chosen: `(path, data URI)` or the reason it was refused;
    /// `None` when the dialog was cancelled.
    IconPicked(Option<Result<(PathBuf, String), String>>),
    DiscoverCategories(Result<Vec<DiscoverCategory>, String>),
    DiscoverPage {
        generation: u64,
        result: Result<DiscoverPage, String>,
    },
    DiscoverJoined {
        guild: GuildId,
        result: Result<(), String>,
    },
    FoldersSaved {
        previous: Folders,
        result: Result<(), String>,
    },
    Voice(super::voice::VoiceDone),
    Profile {
        key: super::profile::Key,
        result: Result<Box<crate::net::cosmetics::Profile>, String>,
    },
    ProfileEffects(Result<Vec<crate::net::cosmetics::ProfileEffect>, String>),
    /// A decoded animation (or `None` when it failed / is not animated).
    Anim {
        key: String,
        class: super::anim::Class,
        anim: Option<std::sync::Arc<super::anim::Anim>>,
    },
    Screenshot(iced::window::Screenshot),
    ScreenshotReady,
    Ignored,
}

impl From<Ui> for Message {
    fn from(u: Ui) -> Self {
        Message::Ui(u)
    }
}
