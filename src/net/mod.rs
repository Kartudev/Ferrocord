//! Discord protocol client: Gateway (WebSocket), REST API, remote-auth (QR
//! login). Owns sockets; never touches UI types.

pub mod captcha;
pub mod cosmetics;
pub mod extras;
pub mod frecency;
pub mod gateway;
pub mod identity;
pub mod model;
pub mod ratelimit;
pub mod remote_auth;
pub mod rest;
pub mod types;
pub mod user_settings;
pub mod voice;
pub mod zlib;

use model::*;

/// Connection state of the gateway, rendered as a banner by the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u32, delay_ms: u64 },
}

/// Events pushed from the gateway to the app.
#[derive(Debug, Clone)]
pub enum Event {
    Connection(ConnectionState),
    Ready(Box<ReadyData>),
    Resumed,
    /// (guild, user, roles) from READY_SUPPLEMENTAL / member chunks.
    MemberRoles(Vec<(GuildId, UserId, Vec<RoleId>)>),
    MessageCreate(Box<Message>),
    MessageUpdate(Box<MessagePatch>),
    MessageDelete {
        channel_id: ChannelId,
        id: MessageId,
    },
    MessageDeleteBulk {
        channel_id: ChannelId,
        ids: Vec<MessageId>,
    },
    TypingStart {
        channel_id: ChannelId,
        guild_id: Option<GuildId>,
        user_id: UserId,
        timestamp_s: u64,
        user: Option<User>,
    },
    MessageAck {
        channel_id: ChannelId,
        message_id: MessageId,
        mention_count: u32,
    },
    ChannelUpsert(Box<Channel>, Vec<User>),
    ChannelDelete(ChannelId),
    GuildUpsert {
        guild: Box<Guild>,
        channels: Vec<Channel>,
        /// GUILD_UPDATE carries no channels; keep the known ones.
        partial: bool,
        /// Users in this server's voice channels (GUILD_CREATE).
        voice_states: Vec<VoiceState>,
    },
    GuildDelete(GuildId),
    /// GUILD_ROLE_CREATE / GUILD_ROLE_UPDATE.
    RoleUpsert {
        guild_id: GuildId,
        role: Role,
    },
    /// GUILD_ROLE_DELETE.
    RoleDelete {
        guild_id: GuildId,
        role_id: RoleId,
    },
    Reaction(ReactionEvent),
    /// PRESENCE_UPDATE (friends, and members of subscribed guilds).
    PresenceUpdate {
        user_id: UserId,
        status: Presence,
        activities: Vec<Activity>,
    },
    MemberList(Box<MemberListUpdate>),
    /// USER_SETTINGS_UPDATE carrying `guild_folders` (another session or
    /// our own PATCH echoed back).
    GuildFolders(Vec<GuildFolder>),
    /// USER_GUILD_SETTINGS_UPDATE: channel overrides of one server (or of
    /// DMs when `guild` is `None`).
    NotifySettings {
        guild: Option<GuildId>,
        channels: Vec<(ChannelId, extras::ChannelNotify)>,
    },
    /// GUILD_EMOJIS_UPDATE.
    GuildEmojis {
        guild_id: GuildId,
        emojis: Vec<CustomEmoji>,
    },
    GuildStickers {
        guild_id: GuildId,
        stickers: Vec<Sticker>,
    },
    /// VOICE_STATE_UPDATE (anyone, including ourselves).
    VoiceState(Box<VoiceState>),
    /// VOICE_SERVER_UPDATE for our own voice session.
    VoiceServer(Box<VoiceServer>),
    /// CALL_CREATE / CALL_UPDATE.
    Call(Box<CallState>),
    CallDelete(ChannelId),
    /// The token was rejected (close code 4004). The session must be cleared.
    AuthFailed,
    /// Unrecoverable gateway error (bad intents, invalid API version, ...).
    Fatal(String),
}
