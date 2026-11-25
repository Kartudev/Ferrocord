//! Domain types shared by `net`, `store` and `app`.
//!
//! These are normalised versions of the wire types in `types.rs`: ids are
//! parsed, optional fields are flattened and user-account quirks (READY
//! deduplication, `properties` objects) are resolved.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use super::cosmetics::{
    Activity, AvatarDecoration, Collectibles, NameStyle, Nameplate, PrimaryGuild,
};

/// Discord epoch (2015-01-01T00:00:00Z) in Unix milliseconds.
pub const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

/// A Discord snowflake id. Serialised as a decimal string like the API does.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Snowflake(pub u64);

impl Snowflake {
    /// Creation time encoded in the id, in Unix milliseconds.
    pub fn timestamp_ms(self) -> u64 {
        (self.0 >> 22) + DISCORD_EPOCH_MS
    }

    /// Builds a synthetic id for a given Unix millisecond timestamp
    /// (used for nonces and mock data).
    pub fn from_timestamp_ms(ms: u64, increment: u64) -> Self {
        Self((ms.saturating_sub(DISCORD_EPOCH_MS) << 22) | (increment & 0xfff))
    }
}

impl fmt::Debug for Snowflake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for Snowflake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for Snowflake {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Snowflake)
    }
}

impl Serialize for Snowflake {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Snowflake {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = Snowflake;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a snowflake as string or integer")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Snowflake, E> {
                v.parse().map(Snowflake).map_err(E::custom)
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Snowflake, E> {
                Ok(Snowflake(v))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Snowflake, E> {
                u64::try_from(v).map(Snowflake).map_err(E::custom)
            }
        }
        d.deserialize_any(V)
    }
}

pub type UserId = Snowflake;
pub type GuildId = Snowflake;
pub type ChannelId = Snowflake;
pub type MessageId = Snowflake;
pub type RoleId = Snowflake;

#[derive(Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub bot: bool,
    /// Frame drawn around the avatar (`avatar_decoration_data`).
    #[serde(
        default,
        rename = "avatar_decoration_data",
        deserialize_with = "super::cosmetics::lenient"
    )]
    pub avatar_decoration: Option<Box<AvatarDecoration>>,
    /// Equipped collectibles (nameplate).
    #[serde(default, deserialize_with = "super::cosmetics::lenient")]
    pub collectibles: Option<Box<Collectibles>>,
    /// Server tag.
    #[serde(default, deserialize_with = "super::cosmetics::lenient")]
    pub primary_guild: Option<Box<PrimaryGuild>>,
    /// Display name font / effect / colours.
    #[serde(
        default,
        rename = "display_name_styles",
        deserialize_with = "super::cosmetics::lenient"
    )]
    pub name_style: Option<Box<NameStyle>>,
    #[serde(default, deserialize_with = "super::cosmetics::lenient")]
    pub banner: Option<String>,
    #[serde(default, deserialize_with = "super::cosmetics::lenient")]
    pub accent_color: Option<u32>,
}

impl fmt::Debug for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Usernames are personal data: only log the id.
        write!(f, "User({})", self.id)
    }
}

impl User {
    /// Name shown in the UI: global display name, falling back to username.
    pub fn display_name(&self) -> &str {
        self.global_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.username)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelKind {
    Text,
    Dm,
    Voice,
    GroupDm,
    Category,
    Announcement,
    Thread,
    Stage,
    Forum,
    Other(u8),
}

impl ChannelKind {
    pub fn from_wire(t: u8) -> Self {
        match t {
            0 => Self::Text,
            1 => Self::Dm,
            2 => Self::Voice,
            3 => Self::GroupDm,
            4 => Self::Category,
            5 => Self::Announcement,
            10..=12 => Self::Thread,
            13 => Self::Stage,
            15 | 16 => Self::Forum,
            o => Self::Other(o),
        }
    }

    pub fn is_private(self) -> bool {
        matches!(self, Self::Dm | Self::GroupDm)
    }

    /// Channels that can hold text messages and are shown in the list.
    pub fn is_textual(self) -> bool {
        matches!(
            self,
            Self::Text | Self::Dm | Self::GroupDm | Self::Announcement | Self::Thread
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub kind: ChannelKind,
    pub guild_id: Option<GuildId>,
    pub name: Option<String>,
    pub topic: Option<String>,
    pub position: i32,
    pub parent_id: Option<ChannelId>,
    pub last_message_id: Option<MessageId>,
    /// DM / group DM participants (excluding the current user).
    pub recipients: Vec<UserId>,
    pub nsfw: bool,
    /// Permission overwrites (server channels only).
    #[serde(default)]
    pub overwrites: Vec<PermissionOverwrite>,
    /// A DM from someone who is not a friend, waiting in Message Requests
    /// (`is_message_request`). Hidden from the DM list until accepted.
    #[serde(default)]
    pub message_request: bool,
    /// A message request Discord filed as spam (`is_spam`).
    #[serde(default)]
    pub spam: bool,
}

/// One channel permission overwrite: a role (or `@everyone`, whose id is
/// the guild id) or a single member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionOverwrite {
    pub id: Snowflake,
    /// `true` = a member, `false` = a role.
    pub member: bool,
    pub allow: u64,
    pub deny: u64,
}

impl Channel {
    /// Name for the UI: channel name, or for DMs the recipients' names.
    pub fn label<'a>(&'a self, user_name: impl Fn(UserId) -> Option<&'a str>) -> String {
        if let Some(n) = self.name.as_deref().filter(|n| !n.is_empty()) {
            return n.to_owned();
        }
        let names: Vec<&str> = self
            .recipients
            .iter()
            .filter_map(|id| user_name(*id))
            .collect();
        if names.is_empty() {
            "Unknown".into()
        } else {
            names.join(", ")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Role {
    pub id: RoleId,
    pub name: String,
    /// RGB colour, 0 = no colour.
    pub color: u32,
    pub position: i32,
    /// Permission bits (server-wide; channel overwrites are not applied).
    #[serde(default)]
    pub permissions: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Guild {
    pub id: GuildId,
    pub name: String,
    pub icon: Option<String>,
    pub roles: Vec<Role>,
    pub owner_id: Option<UserId>,
    /// Custom emoji of the server (for the picker).
    #[serde(default)]
    pub emojis: Vec<CustomEmoji>,
    /// Stickers of the server (for the sticker picker).
    #[serde(default)]
    pub stickers: Vec<Sticker>,
    /// Boost tier (0-3); raises the upload limit.
    #[serde(default)]
    pub premium_tier: u8,
}

impl Guild {
    /// Acronym shown when a guild has no icon ("Rust Programming" -> "RP").
    pub fn acronym(&self) -> String {
        let a: String = self
            .name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(3)
            .collect();
        if a.is_empty() { "?".into() } else { a }
    }
}

/// A server's custom emoji.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomEmoji {
    pub id: Snowflake,
    pub name: String,
    #[serde(default)]
    pub animated: bool,
    #[serde(default = "default_true")]
    pub available: bool,
    /// Roles allowed to use it (empty: everyone).
    #[serde(default)]
    pub roles: Vec<RoleId>,
}

fn default_true() -> bool {
    true
}

/// Image format of a sticker (`format_type` on the wire).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StickerFormat {
    #[default]
    Png,
    Apng,
    /// Vector animation: not rendered, shown as a named placeholder.
    Lottie,
    Gif,
}

impl StickerFormat {
    pub fn from_wire(n: u8) -> Self {
        match n {
            2 => Self::Apng,
            3 => Self::Lottie,
            4 => Self::Gif,
            _ => Self::Png,
        }
    }
}

/// A sticker: one of a server's, or one attached to a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sticker {
    pub id: Snowflake,
    pub name: String,
    pub format: StickerFormat,
    #[serde(default = "default_true")]
    pub available: bool,
}

impl Sticker {
    /// CDN image of the sticker, `None` for Lottie stickers (not rendered).
    /// APNG stickers are served as PNG and show their first frame.
    pub fn image_url(&self, cdn: &str) -> Option<String> {
        let ext = match self.format {
            StickerFormat::Lottie => return None,
            StickerFormat::Gif => "gif",
            StickerFormat::Png | StickerFormat::Apng => "png",
        };
        Some(format!("{cdn}/stickers/{}.{ext}?size=160", self.id))
    }
}

/// An emoji used in a reaction: Unicode (`id == None`, `name` is the
/// character sequence) or custom (`id` set).
#[derive(Debug, Clone, Eq, Serialize, Deserialize)]
pub struct Emoji {
    pub id: Option<Snowflake>,
    pub name: String,
    #[serde(default)]
    pub animated: bool,
}

/// Identity: custom emoji by id, Unicode by sequence (names of custom
/// emoji can change).
impl PartialEq for Emoji {
    fn eq(&self, other: &Self) -> bool {
        match (self.id, other.id) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self.name == other.name,
            _ => false,
        }
    }
}

impl Emoji {
    pub fn unicode(s: &str) -> Self {
        Self {
            id: None,
            name: s.to_owned(),
            animated: false,
        }
    }

    pub fn custom(e: &CustomEmoji) -> Self {
        Self {
            id: Some(e.id),
            name: e.name.clone(),
            animated: e.animated,
        }
    }

    /// Path segment for the reaction endpoints: percent-encoded Unicode, or
    /// `name:id` for custom emoji.
    pub fn api_segment(&self) -> String {
        match self.id {
            Some(id) => format!("{}:{id}", percent_encode(&self.name)),
            None => percent_encode(&self.name),
        }
    }

    /// Text inserted in the composer: the character(s), or `<:name:id>`.
    pub fn composer_text(&self) -> String {
        match self.id {
            Some(id) => format!(
                "<{}:{}:{id}>",
                if self.animated { "a" } else { "" },
                self.name
            ),
            None => self.name.clone(),
        }
    }
}

/// Percent-encodes everything except unreserved characters.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// One reaction pill on a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reaction {
    pub emoji: Emoji,
    pub count: u32,
    /// The current user reacted with this emoji.
    pub me: bool,
}

/// The message a reply points at, as shown in the reply header.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplyPreview {
    pub message_id: Option<MessageId>,
    pub author: Option<User>,
    /// Truncated to [`REPLY_SNIPPET_CHARS`].
    pub content: String,
    pub has_attachment: bool,
    /// The original was deleted (or could not be loaded).
    pub deleted: bool,
}

impl std::fmt::Debug for ReplyPreview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ReplyPreview({:?})", self.message_id)
    }
}

pub const REPLY_SNIPPET_CHARS: usize = 200;

/// Truncates to `max` chars on a char boundary, adding an ellipsis.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    s.chars().take(max).collect::<String>() + "…"
}

impl ReplyPreview {
    pub fn from_message(m: &Message) -> Self {
        Self {
            message_id: Some(m.id),
            author: Some(m.author.clone()),
            content: truncate_chars(&m.content, REPLY_SNIPPET_CHARS),
            has_attachment: !m.attachments.is_empty(),
            deleted: false,
        }
    }

    pub fn deleted(id: Option<MessageId>) -> Self {
        Self {
            message_id: id,
            author: None,
            content: String::new(),
            has_attachment: false,
            deleted: true,
        }
    }
}

/// User status shown as a coloured dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Presence {
    Online,
    Idle,
    Dnd,
    #[default]
    Offline,
}

impl Presence {
    /// Parses the gateway `status` string (`invisible` counts as offline).
    pub fn from_wire(s: &str) -> Self {
        match s {
            "online" => Self::Online,
            "idle" => Self::Idle,
            "dnd" => Self::Dnd,
            _ => Self::Offline,
        }
    }

    /// Value for IDENTIFY / op 3 / user settings.
    pub fn wire(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Idle => "idle",
            Self::Dnd => "dnd",
            Self::Offline => "invisible",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Idle => "Idle",
            Self::Dnd => "Do Not Disturb",
            Self::Offline => "Invisible",
        }
    }

    pub fn is_online(self) -> bool {
        self != Self::Offline
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: Snowflake,
    pub filename: String,
    pub size: u64,
    pub url: String,
    pub proxy_url: Option<String>,
    pub content_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl fmt::Debug for Attachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Attachment({}, {} bytes)", self.id, self.size)
    }
}

impl Attachment {
    pub fn is_image(&self) -> bool {
        self.content_type
            .as_deref()
            .is_some_and(|c| c.starts_with("image/"))
            || [".png", ".jpg", ".jpeg", ".gif", ".webp"]
                .iter()
                .any(|e| self.filename.to_ascii_lowercase().ends_with(e))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EmbedField {
    pub name: String,
    pub value: String,
    pub inline: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EmbedMedia {
    /// The original URL (never fetched).
    pub url: String,
    /// Discord's media proxy URL (`media.discordapp.net`, `images-ext-*`):
    /// the only one that is downloaded.
    #[serde(default)]
    pub proxy_url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl EmbedMedia {
    /// What to download: the proxy URL, else the plain one (which the host
    /// allowlist then accepts only when it is Discord's own CDN).
    pub fn download_url(&self) -> &str {
        self.proxy_url.as_deref().unwrap_or(&self.url)
    }
}

/// `embed.video`: a direct file on the proxy (mp4/webm) or an iframe-only
/// player page (YouTube, Twitch).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EmbedVideo {
    pub url: String,
    #[serde(default)]
    pub proxy_url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl EmbedVideo {
    pub fn download_url(&self) -> &str {
        self.proxy_url.as_deref().unwrap_or(&self.url)
    }
}

/// `MESSAGE_FLAGS`: the embeds of the message are hidden.
pub const FLAG_SUPPRESS_EMBEDS: u64 = 1 << 2;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Embed {
    pub kind: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub color: Option<u32>,
    pub author_name: Option<String>,
    pub author_url: Option<String>,
    /// Author icon: the proxy URL when there is one.
    pub author_icon: Option<String>,
    pub provider_name: Option<String>,
    pub provider_url: Option<String>,
    pub footer: Option<String>,
    /// Footer icon: the proxy URL when there is one.
    pub footer_icon: Option<String>,
    /// `embed.timestamp`, Unix milliseconds.
    pub timestamp_ms: Option<u64>,
    pub fields: Vec<EmbedField>,
    pub image: Option<EmbedMedia>,
    pub thumbnail: Option<EmbedMedia>,
    pub video: Option<EmbedVideo>,
    /// `embed.flags` (unused here, kept for round trips).
    pub flags: u64,
}

impl Embed {
    /// Media to show on its own, without the embed card: GIF links (`gifv`,
    /// e.g. Tenor/Giphy) and direct image links (`image`), as the official
    /// client does. Uses the image, falling back to the thumbnail.
    pub fn bare_media(&self) -> Option<&EmbedMedia> {
        match self.kind.as_deref() {
            Some("gifv" | "image") => self.image.as_ref().or(self.thumbnail.as_ref()),
            _ => None,
        }
    }
}

/// A GIF offered by the GIF picker (Discord proxies Tenor).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gif {
    /// Tenor page URL: what gets sent, Discord turns it into a `gifv` embed.
    pub url: String,
    /// Image shown in the picker (static preview, else the GIF itself).
    pub preview: String,
    /// The GIF file (fallback thumbnail when the preview fails).
    pub gif_src: String,
    /// The file in the requested media format (MP4): what a favourite
    /// stores, like the official client.
    pub src: String,
    pub width: u32,
    pub height: u32,
}

/// Call service message data (message type 3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallMessage {
    pub participants: Vec<UserId>,
    /// When the call ended (Unix ms); `None` while it is ongoing.
    pub ended_ms: Option<u64>,
}

/// A user's voice connection (guild voice channel or DM call).
#[derive(Clone, PartialEq, Default)]
pub struct VoiceState {
    pub guild_id: Option<GuildId>,
    /// `None` = the user left voice.
    pub channel_id: Option<ChannelId>,
    pub user_id: UserId,
    pub session_id: String,
    pub self_mute: bool,
    pub self_deaf: bool,
    /// Server mute / deafen.
    pub mute: bool,
    pub deaf: bool,
    pub self_video: bool,
    pub self_stream: bool,
    pub suppress: bool,
    /// From `member.user`, when the server sent it.
    pub user: Option<User>,
}

impl fmt::Debug for VoiceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceState")
            .field("guild_id", &self.guild_id)
            .field("channel_id", &self.channel_id)
            .field("user_id", &self.user_id)
            .field("self_mute", &self.self_mute)
            .field("self_deaf", &self.self_deaf)
            .finish_non_exhaustive()
    }
}

/// VOICE_SERVER_UPDATE: where to connect for our voice session.
#[derive(Clone, PartialEq, Eq)]
pub struct VoiceServer {
    pub guild_id: Option<GuildId>,
    /// Set for DM / group calls.
    pub channel_id: Option<ChannelId>,
    /// `None` while the server is being reallocated.
    pub endpoint: Option<String>,
    pub token: String,
}

impl fmt::Debug for VoiceServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceServer")
            .field("guild_id", &self.guild_id)
            .field("channel_id", &self.channel_id)
            .field("has_endpoint", &self.endpoint.is_some())
            .finish_non_exhaustive()
    }
}

/// CALL_CREATE / CALL_UPDATE: a DM or group call.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CallState {
    pub channel_id: ChannelId,
    pub message_id: Option<MessageId>,
    /// Users being rung right now.
    pub ringing: Vec<UserId>,
    pub voice_states: Vec<VoiceState>,
    pub unavailable: bool,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub channel_id: ChannelId,
    pub guild_id: Option<GuildId>,
    pub author: User,
    pub content: String,
    pub edited: bool,
    pub attachments: Vec<Attachment>,
    pub embeds: Vec<Embed>,
    /// `MESSAGE_FLAGS` bit field (SUPPRESS_EMBEDS is bit 2).
    #[serde(default)]
    pub flags: u64,
    #[serde(default)]
    pub stickers: Vec<Sticker>,
    pub mentions: Vec<UserId>,
    pub mention_roles: Vec<RoleId>,
    pub mention_everyone: bool,
    /// Role ids of the author in this guild (from `member`), for colour.
    pub author_roles: Vec<RoleId>,
    pub reply_to: Option<MessageId>,
    /// Snapshot of the replied-to message for the reply header.
    #[serde(default)]
    pub referenced: Option<Box<ReplyPreview>>,
    #[serde(default)]
    pub reactions: Vec<Reaction>,
    pub nonce: Option<String>,
    /// Message type (0 = default, 19 = reply, 7 = member join, ...).
    pub kind: u8,
    /// Local-only: optimistic message not yet acknowledged by the server.
    #[serde(default)]
    pub pending: bool,
    /// Local-only: sending failed.
    #[serde(default)]
    pub failed: bool,
    /// Call service message (type 3).
    #[serde(default)]
    pub call: Option<CallMessage>,
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print the body.
        f.debug_struct("Message")
            .field("id", &self.id)
            .field("channel_id", &self.channel_id)
            .field("author", &self.author.id)
            .field("body_len", &self.content.len())
            .finish()
    }
}

impl Message {
    pub fn timestamp_ms(&self) -> u64 {
        self.id.timestamp_ms()
    }

    /// True when the text is nothing but the link of its GIF embed: the
    /// official client then shows only the GIF, not the URL.
    pub fn content_is_gif_link(&self) -> bool {
        let c = self.content.trim();
        !c.is_empty()
            && self
                .embeds
                .iter()
                .any(|e| e.kind.as_deref() == Some("gifv") && e.url.as_deref() == Some(c))
    }

    /// The message's embeds are hidden (its author or a moderator chose so).
    pub fn embeds_suppressed(&self) -> bool {
        self.flags & FLAG_SUPPRESS_EMBEDS != 0
    }

    /// Applies a MESSAGE_UPDATE patch (only the fields that were sent).
    pub fn apply_patch(&mut self, p: &MessagePatch) {
        if let Some(c) = &p.content {
            self.content = c.clone();
        }
        if let Some(e) = &p.embeds {
            self.embeds = e.clone();
        }
        if let Some(a) = &p.attachments {
            self.attachments = a.clone();
        }
        if let Some(f) = p.flags {
            self.flags = f;
        }
        self.edited |= p.edited;
    }
}

/// Partial update from MESSAGE_UPDATE: only fields that were present.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MessagePatch {
    pub id: MessageId,
    pub channel_id: ChannelId,
    pub content: Option<String>,
    pub embeds: Option<Vec<Embed>>,
    pub attachments: Option<Vec<Attachment>>,
    pub flags: Option<u64>,
    pub edited: bool,
}

/// Read state of one channel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadState {
    pub last_read: Option<MessageId>,
    pub mention_count: u32,
}

/// One entry of the user's server list: a folder, or (with `id == None`) a
/// single ungrouped server.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GuildFolder {
    pub id: Option<u64>,
    pub name: Option<String>,
    /// RGB colour.
    pub color: Option<u32>,
    pub guild_ids: Vec<GuildId>,
}

impl GuildFolder {
    pub fn single(g: GuildId) -> Self {
        Self {
            id: None,
            name: None,
            color: None,
            guild_ids: vec![g],
        }
    }
}

/// Kind of a relationship (`type` in READY `relationships` and the
/// `RELATIONSHIP_*` events).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationshipKind {
    Friend,
    Blocked,
    /// A friend request someone sent us.
    Incoming,
    /// A friend request we sent.
    Outgoing,
    /// Someone Discord thinks we know (the "Implicit" tab).
    Implicit,
}

impl RelationshipKind {
    pub fn from_wire(t: u8) -> Option<Self> {
        Some(match t {
            1 => Self::Friend,
            2 => Self::Blocked,
            3 => Self::Incoming,
            4 => Self::Outgoing,
            5 => Self::Implicit,
            _ => return None,
        })
    }
}

/// One entry of the friends list (or a pending request, a block, ...).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub user_id: UserId,
    pub kind: RelationshipKind,
    /// The friend nickname the user set (only they see it).
    pub nickname: Option<String>,
}

impl fmt::Debug for Relationship {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Relationship({}, {:?})", self.user_id, self.kind)
    }
}

/// Everything the client needs from READY (+ READY_SUPPLEMENTAL).
#[derive(Debug, Clone, Default)]
pub struct ReadyData {
    pub me: Option<User>,
    pub session_id: String,
    pub resume_gateway_url: Option<String>,
    pub guilds: Vec<Guild>,
    pub channels: Vec<Channel>,
    pub users: Vec<User>,
    pub read_states: Vec<(ChannelId, ReadState)>,
    /// Guild order from the user's settings (guild folders), if present.
    pub guild_order: Vec<GuildId>,
    /// Role ids of the current user per guild.
    pub my_roles: Vec<(GuildId, Vec<RoleId>)>,
    /// Muted guilds / channels from user guild settings.
    pub muted_guilds: Vec<GuildId>,
    pub muted_channels: Vec<ChannelId>,
    /// Servers with "suppress @everyone and @here" / "suppress role mentions".
    pub suppress_everyone: Vec<GuildId>,
    pub suppress_roles: Vec<GuildId>,
    /// Status of friends from READY `presences`.
    pub presences: Vec<(UserId, Presence)>,
    /// Activities (rich presence, custom status) of those friends.
    pub activities: Vec<(UserId, Vec<Activity>)>,
    /// The user's own status (`user_settings.status`).
    pub my_status: Option<Presence>,
    /// Nitro type of the current user (0 none, 1 classic, 2 nitro, 3 basic).
    pub premium_type: u8,
    /// Server folders from the user settings (empty when only the legacy
    /// `guild_positions` order exists).
    pub guild_folders: Vec<GuildFolder>,
    /// Per-channel notification overrides (level and timed mutes).
    pub channel_notify: Vec<(ChannelId, super::extras::ChannelNotify)>,
    /// Users in guild voice channels.
    pub voice_states: Vec<VoiceState>,
    /// Friends, friend requests, blocks and implicit relationships.
    pub relationships: Vec<Relationship>,
}

/// A member shown in the member list.
#[derive(Clone, PartialEq)]
pub struct MemberEntry {
    pub user: User,
    pub nick: Option<String>,
    pub roles: Vec<RoleId>,
    pub presence: Presence,
    pub activities: Vec<Activity>,
}

impl std::fmt::Debug for MemberEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MemberEntry({})", self.user.id)
    }
}

impl MemberEntry {
    pub fn display_name(&self) -> &str {
        self.nick
            .as_deref()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| self.user.display_name())
    }
}

/// One row of the member list: a group header or a member.
#[derive(Debug, Clone, PartialEq)]
pub enum MemberItem {
    /// `id` is a role id, `online` or `offline`.
    Group {
        id: String,
        count: u32,
    },
    Member(Box<MemberEntry>),
}

/// An operation of GUILD_MEMBER_LIST_UPDATE (indexes are into the flat
/// list of groups and members).
#[derive(Debug, Clone, PartialEq)]
pub enum MemberOp {
    Sync {
        start: usize,
        items: Vec<MemberItem>,
    },
    Insert {
        index: usize,
        item: MemberItem,
    },
    Update {
        index: usize,
        item: MemberItem,
    },
    Delete {
        index: usize,
    },
    Invalidate {
        start: usize,
        end: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemberListUpdate {
    pub guild_id: GuildId,
    /// List id (`everyone` or a hash of the channel permissions).
    pub list_id: String,
    pub member_count: u32,
    pub online_count: u32,
    pub groups: Vec<(String, u32)>,
    pub ops: Vec<MemberOp>,
}

/// MESSAGE_REACTION_* events.
#[derive(Debug, Clone, PartialEq)]
pub enum ReactionEvent {
    Add {
        channel_id: ChannelId,
        message_id: MessageId,
        user_id: UserId,
        emoji: Emoji,
    },
    Remove {
        channel_id: ChannelId,
        message_id: MessageId,
        user_id: UserId,
        emoji: Emoji,
    },
    RemoveAll {
        channel_id: ChannelId,
        message_id: MessageId,
    },
    RemoveEmoji {
        channel_id: ChannelId,
        message_id: MessageId,
        emoji: Emoji,
    },
}

/// One search hit with the channel it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchPage {
    pub total: u32,
    pub messages: Vec<Message>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gif_embed(url: &str) -> Embed {
        Embed {
            kind: Some("gifv".into()),
            url: Some(url.into()),
            provider_name: Some("Tenor".into()),
            thumbnail: Some(EmbedMedia {
                proxy_url: None,
                url: "https://media.example/thumb.png".into(),
                width: Some(220),
                height: Some(165),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn gif_and_image_embeds_render_bare() {
        let gif = gif_embed("https://tenor.com/view/cat-123");
        assert_eq!(
            gif.bare_media().map(|m| m.url.as_str()),
            Some("https://media.example/thumb.png")
        );
        let rich = Embed {
            kind: Some("rich".into()),
            ..gif.clone()
        };
        assert!(rich.bare_media().is_none());
        let no_media = Embed {
            thumbnail: None,
            ..gif
        };
        assert!(no_media.bare_media().is_none());
    }

    #[test]
    fn gif_link_content_is_hidden_only_when_alone() {
        let url = "https://tenor.com/view/cat-123";
        let mut m = crate::mock::text_message(
            Snowflake(1),
            Snowflake(2),
            crate::mock::user(2),
            &format!(" {url} "),
        );
        m.embeds = vec![gif_embed(url)];
        assert!(m.content_is_gif_link());
        m.content = format!("look {url}");
        assert!(!m.content_is_gif_link());
        m.content = String::new();
        assert!(!m.content_is_gif_link());
    }

    #[test]
    fn emoji_identity_and_segments() {
        let a = Emoji::unicode("👍");
        assert_eq!(a.api_segment(), "%F0%9F%91%8D");
        let c = Emoji {
            id: Some(Snowflake(5)),
            name: "party_blob".into(),
            animated: true,
        };
        assert_eq!(c.api_segment(), "party_blob:5");
        assert_eq!(c.composer_text(), "<a:party_blob:5>");
        let renamed = Emoji {
            name: "other".into(),
            ..c.clone()
        };
        assert_eq!(c, renamed);
        assert_ne!(a, Emoji::unicode("👎"));
        assert_ne!(a, c);
    }

    #[test]
    fn truncation_is_char_safe() {
        assert_eq!(truncate_chars("héllo", 10), "héllo");
        assert_eq!(truncate_chars("héllo wörld", 5), "héllo…");
        assert_eq!(truncate_chars("🦀🦀🦀", 2), "🦀🦀…");
    }

    #[test]
    fn presence_wire() {
        assert_eq!(Presence::from_wire("dnd"), Presence::Dnd);
        assert_eq!(Presence::from_wire("invisible"), Presence::Offline);
        assert_eq!(Presence::from_wire("???"), Presence::Offline);
        assert_eq!(Presence::Idle.wire(), "idle");
        assert!(Presence::Online.is_online() && !Presence::Offline.is_online());
    }

    #[test]
    fn snowflake_timestamp() {
        // Example from the Discord docs.
        let id = Snowflake(175928847299117063);
        assert_eq!(id.timestamp_ms(), 1462015105796);
        let back = Snowflake::from_timestamp_ms(1462015105796, 0);
        assert_eq!(back.timestamp_ms(), 1462015105796);
    }

    #[test]
    fn snowflake_serde() {
        let s: Snowflake = serde_json::from_str("\"123\"").unwrap();
        assert_eq!(s, Snowflake(123));
        let n: Snowflake = serde_json::from_str("456").unwrap();
        assert_eq!(n, Snowflake(456));
        assert_eq!(serde_json::to_string(&s).unwrap(), "\"123\"");
    }

    #[test]
    fn message_debug_hides_body() {
        let m = Message {
            id: Snowflake(1),
            channel_id: Snowflake(2),
            guild_id: None,
            author: User {
                id: Snowflake(3),
                username: "alice".into(),
                global_name: None,
                avatar: None,
                bot: false,
                ..Default::default()
            },
            content: "very private words".into(),
            edited: false,
            attachments: vec![],
            embeds: vec![],
            flags: 0,
            stickers: vec![],
            mentions: vec![],
            mention_roles: vec![],
            mention_everyone: false,
            author_roles: vec![],
            reply_to: None,
            referenced: None,
            reactions: vec![],
            nonce: None,
            kind: 0,
            pending: false,
            failed: false,
            call: None,
        };
        let d = format!("{m:?}");
        assert!(!d.contains("private"));
        assert!(!d.contains("alice"));
    }

    #[test]
    fn guild_acronym() {
        let g = Guild {
            id: Snowflake(1),
            name: "Rust Programming Language".into(),
            icon: None,
            roles: vec![],
            owner_id: None,
            emojis: vec![],
            stickers: vec![],
            premium_tier: 0,
        };
        assert_eq!(g.acronym(), "RPL");
    }
}
