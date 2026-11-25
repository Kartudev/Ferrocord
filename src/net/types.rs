//! Wire types (serde) for the Discord Gateway and REST API, and their
//! conversion to the domain types in `model.rs`.
//!
//! Deserialisation is deliberately lenient (`#[serde(default)]` everywhere):
//! the user-account API adds and removes fields often, and an unknown or
//! missing field must never drop an event.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Event;
use super::model::*;

/// Gateway opcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OpCode {
    Dispatch = 0,
    Heartbeat = 1,
    Identify = 2,
    PresenceUpdate = 3,
    VoiceStateUpdate = 4,
    Resume = 6,
    Reconnect = 7,
    RequestGuildMembers = 8,
    InvalidSession = 9,
    Hello = 10,
    HeartbeatAck = 11,
    /// Guild subscriptions (member list ranges, typing, activities).
    GuildSubscriptions = 14,
}

impl OpCode {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Self::Dispatch,
            1 => Self::Heartbeat,
            2 => Self::Identify,
            3 => Self::PresenceUpdate,
            4 => Self::VoiceStateUpdate,
            6 => Self::Resume,
            7 => Self::Reconnect,
            8 => Self::RequestGuildMembers,
            9 => Self::InvalidSession,
            10 => Self::Hello,
            11 => Self::HeartbeatAck,
            14 => Self::GuildSubscriptions,
            _ => return None,
        })
    }
}

/// Raw gateway frame: `{ op, d, s, t }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayPayload {
    pub op: u8,
    #[serde(default)]
    pub d: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t: Option<String>,
}

impl GatewayPayload {
    pub fn new(op: OpCode, d: Value) -> Self {
        Self {
            op: op as u8,
            d,
            s: None,
            t: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Hello {
    pub heartbeat_interval: u64,
}

/// Client properties sent in IDENTIFY (honest: `browser = "Ferrocord"`).
/// The same object is sent as `X-Super-Properties` on REST requests.
pub use super::identity::ClientProperties;

/// Capability bits for user-account IDENTIFY. We do NOT request READY
/// deduplication of users or the lazy "properties" layout, but the parser
/// copes with both in case the server sends them anyway.
pub const CAPABILITIES: u64 = 0;

/// The `activities` of our own presence: the custom status (type 4), as
/// the official client sends it, or nothing.
pub fn own_activities(
    custom: Option<&crate::net::user_settings::CustomStatus>,
) -> serde_json::Value {
    let Some(c) = custom.filter(|c| !c.is_empty()) else {
        return serde_json::json!([]);
    };
    let mut a = serde_json::json!({ "type": 4, "name": "Custom Status" });
    if !c.text.is_empty() {
        a["state"] = c.text.clone().into();
    }
    if c.emoji_id != 0 || !c.emoji_name.is_empty() {
        a["emoji"] = serde_json::json!({
            "id": (c.emoji_id != 0).then(|| c.emoji_id.to_string()),
            "name": c.emoji_name,
            "animated": false,
        });
    }
    serde_json::json!([a])
}

pub fn identify_payload(
    token: &str,
    props: &ClientProperties,
    status: Presence,
    custom: Option<&crate::net::user_settings::CustomStatus>,
) -> GatewayPayload {
    GatewayPayload::new(
        OpCode::Identify,
        serde_json::json!({
            "token": token,
            "capabilities": CAPABILITIES,
            "properties": props,
            "presence": {
                "status": status.wire(),
                "since": 0,
                "activities": own_activities(custom),
                "afk": false,
            },
            "compress": false,
            "client_state": { "guild_versions": {} },
            // Transport compression (zlib-stream) is negotiated in the URL;
            // per-payload compression stays off.
            "large_threshold": 100,
        }),
    )
}

/// Op 3: change the session's own status.
pub fn presence_payload(
    status: Presence,
    custom: Option<&crate::net::user_settings::CustomStatus>,
) -> GatewayPayload {
    GatewayPayload::new(
        OpCode::PresenceUpdate,
        serde_json::json!({
            "status": status.wire(),
            "since": 0,
            "activities": own_activities(custom),
            "afk": false,
        }),
    )
}

/// Op 4: join, move between or leave (`channel: None`) voice channels and
/// DM calls (`guild: None`).
pub fn voice_state_payload(
    guild: Option<GuildId>,
    channel: Option<ChannelId>,
    self_mute: bool,
    self_deaf: bool,
) -> GatewayPayload {
    GatewayPayload::new(
        OpCode::VoiceStateUpdate,
        serde_json::json!({
            "guild_id": guild.map(|g| g.to_string()),
            "channel_id": channel.map(|c| c.to_string()),
            "self_mute": self_mute,
            "self_deaf": self_deaf,
            "self_video": false,
        }),
    )
}

/// Op 14: subscribe to the member list of `guild` for the given inclusive
/// index ranges of `channel`, like the official client does when the member
/// sidebar is open.
pub fn member_subscribe_payload(
    guild: GuildId,
    channel: ChannelId,
    ranges: &[(usize, usize)],
) -> GatewayPayload {
    let ranges: Vec<[usize; 2]> = ranges.iter().map(|(a, b)| [*a, *b]).collect();
    GatewayPayload::new(
        OpCode::GuildSubscriptions,
        serde_json::json!({
            "guild_id": guild.to_string(),
            "typing": true,
            "threads": true,
            "activities": true,
            "members": [],
            "channels": { channel.to_string(): ranges },
        }),
    )
}

pub fn resume_payload(token: &str, session_id: &str, seq: u64) -> GatewayPayload {
    GatewayPayload::new(
        OpCode::Resume,
        serde_json::json!({ "token": token, "session_id": session_id, "seq": seq }),
    )
}

pub fn heartbeat_payload(seq: Option<u64>) -> GatewayPayload {
    GatewayPayload::new(
        OpCode::Heartbeat,
        seq.map(Value::from).unwrap_or(Value::Null),
    )
}

// ---------------------------------------------------------------------------
// Wire structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireUser {
    pub id: Snowflake,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub bot: bool,
    /// Only present on the current user (READY).
    #[serde(default)]
    pub premium_type: u8,
    #[serde(
        default,
        rename = "avatar_decoration_data",
        deserialize_with = "super::cosmetics::lenient"
    )]
    pub avatar_decoration: Option<Box<AvatarDecoration>>,
    #[serde(default, deserialize_with = "super::cosmetics::lenient")]
    pub collectibles: Option<Box<Collectibles>>,
    #[serde(default, deserialize_with = "super::cosmetics::lenient")]
    pub primary_guild: Option<Box<PrimaryGuild>>,
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

impl From<WireUser> for User {
    fn from(w: WireUser) -> Self {
        User {
            id: w.id,
            username: w.username,
            global_name: w.global_name,
            avatar: w.avatar,
            bot: w.bot,
            avatar_decoration: w.avatar_decoration,
            collectibles: w.collectibles,
            primary_guild: w.primary_guild,
            name_style: w.name_style,
            banner: w.banner,
            accent_color: w.accent_color,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireChannel {
    pub id: Snowflake,
    #[serde(rename = "type", default)]
    pub kind: u8,
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub position: Option<i32>,
    #[serde(default)]
    pub parent_id: Option<Snowflake>,
    #[serde(default)]
    pub last_message_id: Option<Snowflake>,
    #[serde(default)]
    pub recipients: Vec<WireUser>,
    #[serde(default)]
    pub recipient_ids: Vec<Snowflake>,
    #[serde(default)]
    pub nsfw: bool,
    #[serde(default)]
    pub permission_overwrites: Vec<WireOverwrite>,
    #[serde(default)]
    pub is_message_request: bool,
    #[serde(default)]
    pub is_spam: bool,
}

/// `permission_overwrites[]` entry. `type` is 0/1 (older payloads:
/// `"role"`/`"member"`); `allow`/`deny` are decimal strings.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireOverwrite {
    pub id: Snowflake,
    #[serde(rename = "type", default)]
    pub kind: serde_json::Value,
    #[serde(default, deserialize_with = "de_permissions")]
    pub allow: u64,
    #[serde(default, deserialize_with = "de_permissions")]
    pub deny: u64,
}

impl From<WireOverwrite> for PermissionOverwrite {
    fn from(w: WireOverwrite) -> Self {
        let member = match &w.kind {
            serde_json::Value::Number(n) => n.as_u64() == Some(1),
            serde_json::Value::String(s) => s == "member" || s == "1",
            _ => false,
        };
        Self {
            id: w.id,
            member,
            allow: w.allow,
            deny: w.deny,
        }
    }
}

impl WireChannel {
    /// Converts to a domain channel; recipient users are returned separately
    /// so they can be put in the user cache.
    pub fn into_domain(self, guild_id: Option<GuildId>) -> (Channel, Vec<User>) {
        let mut recipients: Vec<UserId> = self.recipients.iter().map(|u| u.id).collect();
        for id in &self.recipient_ids {
            if !recipients.contains(id) {
                recipients.push(*id);
            }
        }
        let users = self.recipients.into_iter().map(User::from).collect();
        (
            Channel {
                id: self.id,
                kind: ChannelKind::from_wire(self.kind),
                guild_id: self.guild_id.or(guild_id),
                name: self.name,
                topic: self.topic,
                position: self.position.unwrap_or(0),
                parent_id: self.parent_id,
                last_message_id: self.last_message_id.filter(|s| s.0 != 0),
                recipients,
                nsfw: self.nsfw,
                overwrites: self
                    .permission_overwrites
                    .into_iter()
                    .take(1000)
                    .map(Into::into)
                    .collect(),
                message_request: self.is_message_request,
                spam: self.is_spam,
            },
            users,
        )
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireRole {
    pub id: Snowflake,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub color: u32,
    #[serde(default)]
    pub position: i32,
    #[serde(default, deserialize_with = "de_permissions")]
    pub permissions: u64,
}

impl From<WireRole> for Role {
    fn from(r: WireRole) -> Self {
        Self {
            id: r.id,
            name: r.name,
            color: r.color,
            position: r.position,
            permissions: r.permissions,
        }
    }
}

/// GUILD_ROLE_CREATE / GUILD_ROLE_UPDATE payload.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireRoleEvent {
    pub guild_id: Snowflake,
    pub role: WireRole,
}

/// GUILD_ROLE_DELETE payload.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireRoleDelete {
    pub guild_id: Snowflake,
    pub role_id: Snowflake,
}

/// GUILD_MEMBER_UPDATE payload (only the parts we keep).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireMemberUpdate {
    pub guild_id: Snowflake,
    #[serde(default)]
    pub roles: Vec<Snowflake>,
    #[serde(default)]
    pub user: Option<WireUser>,
}

/// Permission bits arrive as a decimal string (older payloads: a number).
fn de_permissions<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    use serde::Deserialize;
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(s) => s.parse().unwrap_or(0),
        serde_json::Value::Number(n) => n.as_u64().unwrap_or(0),
        _ => 0,
    })
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireGuildProperties {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub owner_id: Option<Snowflake>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireGuild {
    pub id: Snowflake,
    #[serde(default)]
    pub unavailable: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub owner_id: Option<Snowflake>,
    /// User accounts with the "lazy guild" capability receive name/icon here.
    #[serde(default)]
    pub properties: Option<WireGuildProperties>,
    #[serde(default)]
    pub channels: Vec<WireChannel>,
    #[serde(default)]
    pub threads: Vec<WireChannel>,
    #[serde(default)]
    pub roles: Vec<WireRole>,
    #[serde(default)]
    pub emojis: Vec<CustomEmoji>,
    #[serde(default)]
    pub stickers: Vec<WireSticker>,
    #[serde(default)]
    pub premium_tier: u8,
    /// Users in this server's voice channels (READY / GUILD_CREATE).
    #[serde(default)]
    pub voice_states: Vec<WireVoiceState>,
}

impl WireGuild {
    /// Takes the voice states out (before `into_domain`).
    pub fn take_voice_states(&mut self) -> Vec<VoiceState> {
        let id = self.id;
        std::mem::take(&mut self.voice_states)
            .into_iter()
            .take(5000)
            .map(|v| v.into_domain(Some(id)))
            .filter(|v| v.channel_id.is_some())
            .collect()
    }
}

/// A sticker as sent in guild objects and `sticker_items` of messages.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireSticker {
    pub id: Snowflake,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub format_type: u8,
    #[serde(default = "default_true")]
    pub available: bool,
}

fn default_true() -> bool {
    true
}

impl From<WireSticker> for Sticker {
    fn from(w: WireSticker) -> Self {
        Sticker {
            id: w.id,
            name: w.name,
            format: StickerFormat::from_wire(w.format_type),
            available: w.available,
        }
    }
}

impl WireGuild {
    pub fn into_domain(self) -> (Guild, Vec<Channel>) {
        let props = self.properties.unwrap_or_default();
        let premium_tier = self.premium_tier;
        let guild = Guild {
            id: self.id,
            name: self.name.or(props.name).unwrap_or_default(),
            icon: self.icon.or(props.icon),
            roles: self.roles.into_iter().map(Into::into).collect(),
            owner_id: self.owner_id.or(props.owner_id),
            emojis: self.emojis,
            stickers: self.stickers.into_iter().map(Into::into).collect(),
            premium_tier,
        };
        let gid = guild.id;
        let channels = self
            .channels
            .into_iter()
            .chain(self.threads)
            .map(|c| c.into_domain(Some(gid)).0)
            .collect();
        (guild, channels)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireAttachment {
    pub id: Snowflake,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

impl From<WireAttachment> for Attachment {
    fn from(w: WireAttachment) -> Self {
        Attachment {
            id: w.id,
            filename: w.filename,
            size: w.size,
            url: w.url,
            proxy_url: w.proxy_url,
            content_type: w.content_type,
            width: w.width,
            height: w.height,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireEmbedMedia {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

impl From<WireEmbedMedia> for EmbedMedia {
    fn from(w: WireEmbedMedia) -> Self {
        EmbedMedia {
            url: w.url,
            proxy_url: w.proxy_url,
            width: w.width,
            height: w.height,
        }
    }
}

impl From<WireEmbedMedia> for EmbedVideo {
    fn from(w: WireEmbedMedia) -> Self {
        EmbedVideo {
            url: w.url,
            proxy_url: w.proxy_url,
            width: w.width,
            height: w.height,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireNamed {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub proxy_icon_url: Option<String>,
}

impl WireNamed {
    /// The icon to download: the proxy URL when present.
    fn icon(&self) -> Option<String> {
        self.proxy_icon_url
            .clone()
            .or_else(|| self.icon_url.clone())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireEmbedField {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub inline: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireEmbed {
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub color: Option<u32>,
    #[serde(default)]
    pub author: Option<WireNamed>,
    #[serde(default)]
    pub provider: Option<WireNamed>,
    #[serde(default)]
    pub footer: Option<WireNamed>,
    #[serde(default)]
    pub fields: Vec<WireEmbedField>,
    #[serde(default)]
    pub image: Option<WireEmbedMedia>,
    #[serde(default)]
    pub thumbnail: Option<WireEmbedMedia>,
    #[serde(default)]
    pub video: Option<WireEmbedMedia>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub flags: Option<u64>,
}

impl From<WireEmbed> for Embed {
    fn from(w: WireEmbed) -> Self {
        Embed {
            kind: w.kind,
            title: w.title,
            description: w.description,
            url: w.url,
            color: w.color,
            author_name: w.author.as_ref().and_then(|a| a.name.clone()),
            author_icon: w.author.as_ref().and_then(WireNamed::icon),
            author_url: w.author.and_then(|a| a.url),
            provider_url: w.provider.as_ref().and_then(|p| p.url.clone()),
            provider_name: w.provider.and_then(|p| p.name),
            footer_icon: w.footer.as_ref().and_then(WireNamed::icon),
            footer: w.footer.and_then(|f| f.text),
            timestamp_ms: w.timestamp.as_deref().and_then(parse_rfc3339_ms),
            video: w.video.map(Into::into),
            flags: w.flags.unwrap_or(0),
            fields: w
                .fields
                .into_iter()
                .map(|f| EmbedField {
                    name: f.name,
                    value: f.value,
                    inline: f.inline,
                })
                .collect(),
            image: w.image.map(Into::into),
            thumbnail: w.thumbnail.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireMember {
    #[serde(default)]
    pub roles: Vec<Snowflake>,
    #[serde(default)]
    pub user: Option<WireUser>,
    #[serde(default)]
    pub user_id: Option<Snowflake>,
    #[serde(default)]
    pub nick: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireMessageRef {
    #[serde(default)]
    pub message_id: Option<Snowflake>,
}

fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::deserialize(d)?))
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireEmoji {
    #[serde(default)]
    pub id: Option<Snowflake>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub animated: bool,
}

impl From<WireEmoji> for Emoji {
    fn from(w: WireEmoji) -> Self {
        Emoji {
            id: w.id,
            // Deleted custom emoji have a null name.
            name: w.name.unwrap_or_default(),
            animated: w.animated,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireReaction {
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub me: bool,
    #[serde(default)]
    pub emoji: WireEmoji,
}

/// Nonces can be strings or integers on the wire.
fn nonce_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireMessage {
    pub id: Snowflake,
    pub channel_id: Snowflake,
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    #[serde(default)]
    pub author: Option<WireUser>,
    #[serde(default)]
    pub member: Option<WireMember>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub edited_timestamp: Option<String>,
    #[serde(default)]
    pub attachments: Option<Vec<WireAttachment>>,
    #[serde(default)]
    pub embeds: Option<Vec<WireEmbed>>,
    #[serde(default)]
    pub flags: Option<u64>,
    #[serde(default)]
    pub sticker_items: Option<Vec<WireSticker>>,
    #[serde(default)]
    pub mentions: Vec<WireUser>,
    #[serde(default)]
    pub mention_roles: Vec<Snowflake>,
    #[serde(default)]
    pub mention_everyone: bool,
    #[serde(default)]
    pub message_reference: Option<WireMessageRef>,
    /// `Some(None)` = explicitly null (original deleted); `None` = absent.
    #[serde(default, deserialize_with = "double_option")]
    pub referenced_message: Option<Option<Box<WireMessage>>>,
    #[serde(default)]
    pub reactions: Vec<WireReaction>,
    #[serde(default)]
    pub nonce: Value,
    #[serde(rename = "type", default)]
    pub kind: u8,
    #[serde(default)]
    pub call: Option<WireCallMessage>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireCallMessage {
    #[serde(default)]
    pub participants: Vec<Snowflake>,
    #[serde(default)]
    pub ended_timestamp: Option<String>,
}

/// RFC 3339 timestamp to Unix milliseconds.
pub fn parse_iso_ms(s: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .and_then(|t| u64::try_from(t.timestamp_millis()).ok())
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireVoiceState {
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    #[serde(default)]
    pub channel_id: Option<Snowflake>,
    pub user_id: Snowflake,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub self_mute: bool,
    #[serde(default)]
    pub self_deaf: bool,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub deaf: bool,
    #[serde(default)]
    pub self_video: bool,
    #[serde(default)]
    pub self_stream: Option<bool>,
    #[serde(default)]
    pub suppress: bool,
    #[serde(default)]
    pub member: Option<WireMember>,
}

impl WireVoiceState {
    pub fn into_domain(self, guild: Option<Snowflake>) -> VoiceState {
        VoiceState {
            guild_id: self.guild_id.or(guild),
            channel_id: self.channel_id,
            user_id: self.user_id,
            session_id: self.session_id.unwrap_or_default(),
            self_mute: self.self_mute,
            self_deaf: self.self_deaf,
            mute: self.mute,
            deaf: self.deaf,
            self_video: self.self_video,
            self_stream: self.self_stream.unwrap_or(false),
            suppress: self.suppress,
            user: self.member.and_then(|m| m.user).map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireVoiceServer {
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    #[serde(default)]
    pub channel_id: Option<Snowflake>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub token: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireCall {
    pub channel_id: Snowflake,
    #[serde(default)]
    pub message_id: Option<Snowflake>,
    #[serde(default)]
    pub ringing: Vec<Snowflake>,
    #[serde(default)]
    pub voice_states: Vec<WireVoiceState>,
    #[serde(default)]
    pub unavailable: bool,
}

impl WireMessage {
    pub fn into_domain(self) -> Message {
        let reply_to = self.message_reference.as_ref().and_then(|r| r.message_id);
        // Only real replies (type 19) get a header; other types can carry a
        // reference too (crossposts, pins).
        let referenced = if self.kind == 19 {
            match self.referenced_message {
                Some(Some(r)) => {
                    let r = r.into_domain();
                    Some(Box::new(ReplyPreview::from_message(&r)))
                }
                // Explicit null or absent: the original is gone or unknown.
                _ => Some(Box::new(ReplyPreview::deleted(reply_to))),
            }
        } else {
            None
        };
        let reactions = self
            .reactions
            .into_iter()
            .map(|r| Reaction {
                emoji: r.emoji.into(),
                count: r.count,
                me: r.me,
            })
            .collect();
        Message {
            id: self.id,
            channel_id: self.channel_id,
            guild_id: self.guild_id,
            author: self.author.map(User::from).unwrap_or(User {
                id: Snowflake(0),
                username: "Unknown".into(),
                global_name: None,
                avatar: None,
                bot: false,
                ..Default::default()
            }),
            content: self.content.unwrap_or_default(),
            edited: self.edited_timestamp.is_some(),
            attachments: self
                .attachments
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
            embeds: self
                .embeds
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
            flags: self.flags.unwrap_or(0),
            stickers: self
                .sticker_items
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
            mentions: self.mentions.iter().map(|u| u.id).collect(),
            mention_roles: self.mention_roles,
            mention_everyone: self.mention_everyone,
            author_roles: self.member.map(|m| m.roles).unwrap_or_default(),
            reply_to,
            referenced,
            reactions,
            nonce: nonce_to_string(&self.nonce),
            kind: self.kind,
            pending: false,
            failed: false,
            call: self.call.map(|c| CallMessage {
                participants: c.participants.into_iter().take(100).collect(),
                ended_ms: c.ended_timestamp.as_deref().and_then(parse_iso_ms),
            }),
        }
    }

    pub fn into_patch(self) -> MessagePatch {
        MessagePatch {
            id: self.id,
            channel_id: self.channel_id,
            edited: self.edited_timestamp.is_some(),
            content: self.content,
            embeds: self.embeds.map(|v| v.into_iter().map(Into::into).collect()),
            attachments: self
                .attachments
                .map(|v| v.into_iter().map(Into::into).collect()),
            flags: self.flags,
        }
    }
}

/// ISO-8601 / RFC 3339 instant to Unix milliseconds (`None` when malformed
/// or before 1970).
pub fn parse_rfc3339_ms(s: &str) -> Option<u64> {
    if s.len() > 64 {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .and_then(|t| u64::try_from(t.timestamp_millis()).ok())
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireReadStateEntry {
    /// Channel id (for read_state_type 0).
    pub id: Snowflake,
    #[serde(default)]
    pub last_message_id: Option<Snowflake>,
    #[serde(default)]
    pub mention_count: u32,
    #[serde(default)]
    pub read_state_type: u8,
}

/// `read_state` is a plain array on old API versions and
/// `{ entries, partial, version }` on newer ones.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WireReadStates {
    List(Vec<WireReadStateEntry>),
    Versioned {
        #[serde(default)]
        entries: Vec<WireReadStateEntry>,
    },
}

impl Default for WireReadStates {
    fn default() -> Self {
        Self::List(vec![])
    }
}

impl WireReadStates {
    fn entries(self) -> Vec<WireReadStateEntry> {
        match self {
            Self::List(v) => v,
            Self::Versioned { entries } => entries,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireChannelOverride {
    pub channel_id: Snowflake,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub message_notifications: Option<i64>,
    #[serde(default)]
    pub mute_config: Option<WireMuteConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireMuteConfig {
    #[serde(default)]
    pub end_time: Option<String>,
}

impl WireChannelOverride {
    pub fn notify(&self) -> super::extras::ChannelNotify {
        super::extras::ChannelNotify {
            level: super::extras::NotifyLevel::from_wire(self.message_notifications),
            muted: self.muted,
            mute_end_ms: self
                .mute_config
                .as_ref()
                .and_then(|c| c.end_time.as_deref())
                .and_then(super::extras::parse_iso_ms),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireGuildSettingsEntry {
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub suppress_everyone: bool,
    #[serde(default)]
    pub suppress_roles: bool,
    #[serde(default)]
    pub channel_overrides: Vec<WireChannelOverride>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WireGuildSettings {
    List(Vec<WireGuildSettingsEntry>),
    Versioned {
        #[serde(default)]
        entries: Vec<WireGuildSettingsEntry>,
    },
}

impl Default for WireGuildSettings {
    fn default() -> Self {
        Self::List(vec![])
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireGuildFolder {
    #[serde(default)]
    pub guild_ids: Vec<Snowflake>,
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub color: Option<u64>,
}

impl From<WireGuildFolder> for GuildFolder {
    fn from(f: WireGuildFolder) -> Self {
        Self {
            id: f.id,
            name: f.name.filter(|n| !n.is_empty()),
            color: f.color.map(|c| (c & 0xFF_FFFF) as u32),
            guild_ids: f.guild_ids,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WirePresence {
    #[serde(default)]
    pub user: Option<WireIdOnlyOpt>,
    #[serde(default)]
    pub user_id: Option<Snowflake>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub activities: Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireIdOnlyOpt {
    #[serde(default)]
    pub id: Option<Snowflake>,
}

impl WirePresence {
    fn user_id(&self) -> Option<UserId> {
        self.user_id.or(self.user.as_ref().and_then(|u| u.id))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireUserSettings {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub guild_positions: Vec<Snowflake>,
    #[serde(default)]
    pub guild_folders: Vec<WireGuildFolder>,
}

/// An entry of READY `relationships` / a `RELATIONSHIP_*` payload.
#[derive(Debug, Clone, Deserialize)]
pub struct WireRelationship {
    /// The other user's id.
    pub id: Snowflake,
    #[serde(rename = "type", default)]
    pub kind: u8,
    #[serde(default)]
    pub nickname: Option<String>,
    /// Inline user (absent with READY user deduplication).
    #[serde(default)]
    pub user: Option<WireUser>,
}

impl WireRelationship {
    /// The domain relationship (`None` for an unknown type) and its user.
    pub fn into_domain(self) -> Option<(Relationship, Option<User>)> {
        let kind = RelationshipKind::from_wire(self.kind)?;
        Some((
            Relationship {
                user_id: self.id,
                kind,
                nickname: self.nickname.filter(|n| !n.is_empty()),
            },
            self.user.map(User::from),
        ))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireReady {
    pub user: WireUser,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub resume_gateway_url: Option<String>,
    #[serde(default)]
    pub guilds: Vec<WireGuild>,
    #[serde(default)]
    pub private_channels: Vec<WireChannel>,
    /// Present when the server deduplicates users.
    #[serde(default)]
    pub users: Vec<WireUser>,
    #[serde(default)]
    pub read_state: WireReadStates,
    #[serde(default)]
    pub user_guild_settings: WireGuildSettings,
    #[serde(default)]
    pub user_settings: Option<WireUserSettings>,
    /// Friends' presences (user accounts).
    #[serde(default)]
    pub presences: Vec<WirePresence>,
    /// One array per guild (same order as `guilds`) with the current user's
    /// member object.
    #[serde(default)]
    pub merged_members: Vec<Vec<WireMember>>,
    #[serde(default)]
    pub relationships: Vec<WireRelationship>,
}

impl WireReady {
    pub fn into_domain(self) -> ReadyData {
        let premium_type = self.user.premium_type;
        let me: User = self.user.into();
        let presences: Vec<(UserId, Presence)> = self
            .presences
            .iter()
            .filter_map(|p| Some((p.user_id()?, Presence::from_wire(&p.status))))
            .collect();
        let activities: Vec<(UserId, Vec<Activity>)> = self
            .presences
            .iter()
            .filter_map(|p| {
                let a = super::cosmetics::parse_activities(&p.activities);
                (!a.is_empty()).then(|| Some((p.user_id()?, a))).flatten()
            })
            .collect();
        let my_status = self
            .user_settings
            .as_ref()
            .and_then(|s| s.status.as_deref())
            .map(Presence::from_wire);
        let mut users: Vec<User> = self.users.into_iter().map(User::from).collect();
        let mut channels = Vec::new();
        let mut guilds = Vec::new();
        let mut my_roles = Vec::new();

        let mut voice_states = Vec::new();
        for (idx, mut g) in self.guilds.into_iter().enumerate() {
            if g.unavailable && g.name.is_none() && g.properties.is_none() {
                continue;
            }
            voice_states.extend(g.take_voice_states());
            let (guild, chans) = g.into_domain();
            if let Some(members) = self.merged_members.get(idx)
                && let Some(m) = members.iter().find(|m| {
                    m.user_id == Some(me.id) || m.user.as_ref().map(|u| u.id) == Some(me.id)
                })
            {
                my_roles.push((guild.id, m.roles.clone()));
            }
            channels.extend(chans);
            guilds.push(guild);
        }
        let mut relationships = Vec::new();
        for r in self.relationships {
            let Some((rel, user)) = r.into_domain() else {
                continue;
            };
            if let Some(u) = user
                && !users.iter().any(|x| x.id == u.id)
            {
                users.push(u);
            }
            relationships.push(rel);
        }
        for pc in self.private_channels {
            let (c, us) = pc.into_domain(None);
            channels.push(c);
            for u in us {
                if !users.iter().any(|x| x.id == u.id) {
                    users.push(u);
                }
            }
        }
        let read_states = self
            .read_state
            .entries()
            .into_iter()
            .filter(|e| e.read_state_type == 0)
            .map(|e| {
                (
                    e.id,
                    ReadState {
                        last_read: e.last_message_id.filter(|s| s.0 != 0),
                        mention_count: e.mention_count,
                    },
                )
            })
            .collect();

        let settings_entries = match self.user_guild_settings {
            WireGuildSettings::List(v) => v,
            WireGuildSettings::Versioned { entries } => entries,
        };
        let mut muted_guilds = Vec::new();
        let mut muted_channels = Vec::new();
        let mut suppress_everyone = Vec::new();
        let mut suppress_roles = Vec::new();
        let mut channel_notify = Vec::new();
        let now = unix_ms();
        for e in settings_entries {
            if let Some(g) = e.guild_id {
                if e.suppress_everyone {
                    suppress_everyone.push(g);
                }
                if e.suppress_roles {
                    suppress_roles.push(g);
                }
            }
            if e.muted
                && let Some(g) = e.guild_id
            {
                muted_guilds.push(g);
            }
            for o in &e.channel_overrides {
                let n = o.notify();
                if n.is_muted(now) {
                    muted_channels.push(o.channel_id);
                }
                if !n.is_default() {
                    channel_notify.push((o.channel_id, n));
                }
            }
        }

        let (guild_order, guild_folders) = match self.user_settings {
            Some(s) if !s.guild_folders.is_empty() => {
                let folders: Vec<GuildFolder> =
                    s.guild_folders.into_iter().map(GuildFolder::from).collect();
                let order = folders
                    .iter()
                    .flat_map(|f| f.guild_ids.iter().copied())
                    .collect();
                (order, folders)
            }
            Some(s) => (s.guild_positions, vec![]),
            None => (vec![], vec![]),
        };

        ReadyData {
            me: Some(me),
            session_id: self.session_id,
            resume_gateway_url: self.resume_gateway_url,
            guilds,
            channels,
            users,
            read_states,
            guild_order,
            my_roles,
            muted_guilds,
            muted_channels,
            suppress_everyone,
            suppress_roles,
            presences,
            activities,
            my_status,
            premium_type,
            guild_folders,
            channel_notify,
            voice_states,
            relationships,
        }
    }
}

/// Current Unix time in milliseconds.
pub fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireTypingStart {
    pub channel_id: Snowflake,
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    pub user_id: Snowflake,
    #[serde(default)]
    pub timestamp: u64,
    #[serde(default)]
    pub member: Option<WireMember>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireMessageDelete {
    pub id: Snowflake,
    pub channel_id: Snowflake,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireMessageDeleteBulk {
    pub ids: Vec<Snowflake>,
    pub channel_id: Snowflake,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireMessageAck {
    pub channel_id: Snowflake,
    pub message_id: Snowflake,
    #[serde(default)]
    pub mention_count: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireIdOnly {
    pub id: Snowflake,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireReadySupplemental {
    #[serde(default)]
    pub merged_members: Vec<Vec<WireMember>>,
    #[serde(default)]
    pub guilds: Vec<WireIdOnly>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireReactionEvent {
    pub channel_id: Snowflake,
    pub message_id: Snowflake,
    #[serde(default)]
    pub user_id: Option<Snowflake>,
    #[serde(default)]
    pub emoji: WireEmoji,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WirePresenceUpdate {
    #[serde(default)]
    pub user: Option<WireIdOnlyOpt>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub activities: Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireStickersUpdate {
    pub guild_id: Snowflake,
    #[serde(default)]
    pub stickers: Vec<WireSticker>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireEmojisUpdate {
    pub guild_id: Snowflake,
    #[serde(default)]
    pub emojis: Vec<CustomEmoji>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireListGroup {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub count: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireListMember {
    #[serde(default)]
    pub user: Option<WireUser>,
    #[serde(default)]
    pub nick: Option<String>,
    #[serde(default)]
    pub roles: Vec<Snowflake>,
    #[serde(default)]
    pub presence: Option<WirePresenceUpdate>,
    /// The member's server-specific decoration (overrides the user's).
    #[serde(default, deserialize_with = "super::cosmetics::lenient")]
    pub avatar_decoration_data: Option<Box<AvatarDecoration>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireListItem {
    #[serde(default)]
    pub group: Option<WireListGroup>,
    #[serde(default)]
    pub member: Option<WireListMember>,
}

impl WireListItem {
    /// Items without a recognisable shape are dropped (`None`).
    fn into_domain(self) -> Option<MemberItem> {
        if let Some(g) = self.group {
            return Some(MemberItem::Group {
                id: g.id,
                count: g.count,
            });
        }
        let m = self.member?;
        let mut user: User = m.user?.into();
        if m.avatar_decoration_data.is_some() {
            user.avatar_decoration = m.avatar_decoration_data;
        }
        let activities = m
            .presence
            .as_ref()
            .map(|p| super::cosmetics::parse_activities(&p.activities))
            .unwrap_or_default();
        Some(MemberItem::Member(Box::new(MemberEntry {
            user,
            nick: m.nick,
            roles: m.roles,
            presence: m
                .presence
                .map(|p| Presence::from_wire(&p.status))
                .unwrap_or(Presence::Offline),
            activities,
        })))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireListOp {
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    pub range: Option<[usize; 2]>,
    #[serde(default)]
    pub index: Option<usize>,
    #[serde(default)]
    pub items: Vec<WireListItem>,
    #[serde(default)]
    pub item: Option<WireListItem>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireMemberListUpdate {
    pub guild_id: Snowflake,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub member_count: u32,
    #[serde(default)]
    pub online_count: u32,
    #[serde(default)]
    pub groups: Vec<WireListGroup>,
    #[serde(default)]
    pub ops: Vec<WireListOp>,
}

/// Hard cap on operations / items processed from one update.
const MAX_LIST_OPS: usize = 256;
const MAX_LIST_ITEMS: usize = 1000;

impl WireMemberListUpdate {
    pub fn into_domain(self) -> MemberListUpdate {
        let mut ops = Vec::new();
        for op in self.ops.into_iter().take(MAX_LIST_OPS) {
            match (op.op.as_str(), op.range, op.index) {
                ("SYNC", Some([a, _]), _) => ops.push(MemberOp::Sync {
                    start: a,
                    // Unknown items keep their slot as an empty group so
                    // later indexes stay aligned.
                    items: op
                        .items
                        .into_iter()
                        .take(MAX_LIST_ITEMS)
                        .map(|i| {
                            i.into_domain().unwrap_or(MemberItem::Group {
                                id: String::new(),
                                count: 0,
                            })
                        })
                        .collect(),
                }),
                ("INSERT", _, Some(index)) => {
                    if let Some(item) = op.item.and_then(WireListItem::into_domain) {
                        ops.push(MemberOp::Insert { index, item });
                    }
                }
                ("UPDATE", _, Some(index)) => {
                    if let Some(item) = op.item.and_then(WireListItem::into_domain) {
                        ops.push(MemberOp::Update { index, item });
                    }
                }
                ("DELETE", _, Some(index)) => ops.push(MemberOp::Delete { index }),
                ("INVALIDATE", Some([a, b]), _) => {
                    ops.push(MemberOp::Invalidate { start: a, end: b })
                }
                _ => {}
            }
        }
        MemberListUpdate {
            guild_id: self.guild_id,
            list_id: self.id,
            member_count: self.member_count,
            online_count: self.online_count,
            groups: self.groups.into_iter().map(|g| (g.id, g.count)).collect(),
            ops,
        }
    }
}

/// Converts a DISPATCH (`op 0`) frame into a client event.
///
/// Returns `Ok(None)` for events the client does not use. A malformed
/// payload for a known event is an error (logged by the caller; the
/// connection stays up).
pub fn parse_dispatch(t: &str, d: Value) -> Result<Option<Event>, serde_json::Error> {
    Ok(Some(match t {
        "READY" => {
            let r: WireReady = serde_json::from_value(d)?;
            Event::Ready(Box::new(r.into_domain()))
        }
        "READY_SUPPLEMENTAL" => {
            let r: WireReadySupplemental = serde_json::from_value(d)?;
            let mut roles = Vec::new();
            for (idx, g) in r.guilds.iter().enumerate() {
                if let Some(ms) = r.merged_members.get(idx) {
                    for m in ms {
                        if let Some(uid) = m.user_id.or(m.user.as_ref().map(|u| u.id)) {
                            roles.push((g.id, uid, m.roles.clone()));
                        }
                    }
                }
            }
            Event::MemberRoles(roles)
        }
        "USER_SETTINGS_UPDATE" => {
            // Legacy settings: only the server folders matter here (the
            // status echo is handled through presence).
            match d.get("guild_folders") {
                Some(v) => {
                    let f: Vec<WireGuildFolder> = serde_json::from_value(v.clone())?;
                    Event::GuildFolders(f.into_iter().map(GuildFolder::from).collect())
                }
                None => return Ok(None),
            }
        }
        "USER_GUILD_SETTINGS_UPDATE" => {
            let e: WireGuildSettingsEntry = serde_json::from_value(d)?;
            Event::NotifySettings {
                guild: e.guild_id,
                channels: e
                    .channel_overrides
                    .iter()
                    .map(|o| (o.channel_id, o.notify()))
                    .collect(),
            }
        }
        "RESUMED" => Event::Resumed,
        "MESSAGE_CREATE" => {
            let m: WireMessage = serde_json::from_value(d)?;
            Event::MessageCreate(Box::new(m.into_domain()))
        }
        "MESSAGE_UPDATE" => {
            let m: WireMessage = serde_json::from_value(d)?;
            Event::MessageUpdate(Box::new(m.into_patch()))
        }
        "MESSAGE_DELETE" => {
            let m: WireMessageDelete = serde_json::from_value(d)?;
            Event::MessageDelete {
                channel_id: m.channel_id,
                id: m.id,
            }
        }
        "MESSAGE_DELETE_BULK" => {
            let m: WireMessageDeleteBulk = serde_json::from_value(d)?;
            Event::MessageDeleteBulk {
                channel_id: m.channel_id,
                ids: m.ids,
            }
        }
        "TYPING_START" => {
            let t: WireTypingStart = serde_json::from_value(d)?;
            Event::TypingStart {
                channel_id: t.channel_id,
                guild_id: t.guild_id,
                user_id: t.user_id,
                timestamp_s: t.timestamp,
                user: t.member.and_then(|m| m.user).map(Into::into),
            }
        }
        "MESSAGE_ACK" => {
            let a: WireMessageAck = serde_json::from_value(d)?;
            Event::MessageAck {
                channel_id: a.channel_id,
                message_id: a.message_id,
                mention_count: a.mention_count.unwrap_or(0),
            }
        }
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "THREAD_CREATE" | "THREAD_UPDATE" => {
            let c: WireChannel = serde_json::from_value(d)?;
            let (c, users) = c.into_domain(None);
            Event::ChannelUpsert(Box::new(c), users)
        }
        "CHANNEL_DELETE" | "THREAD_DELETE" => {
            let c: WireIdOnly = serde_json::from_value(d)?;
            Event::ChannelDelete(c.id)
        }
        "GUILD_CREATE" | "GUILD_UPDATE" => {
            let mut g: WireGuild = serde_json::from_value(d)?;
            if g.unavailable && g.name.is_none() && g.properties.is_none() {
                return Ok(None);
            }
            let partial = t == "GUILD_UPDATE";
            let voice_states = g.take_voice_states();
            let (guild, channels) = g.into_domain();
            Event::GuildUpsert {
                guild: Box::new(guild),
                channels,
                partial,
                voice_states,
            }
        }
        "GUILD_DELETE" => {
            let g: WireIdOnly = serde_json::from_value(d)?;
            Event::GuildDelete(g.id)
        }
        "MESSAGE_REACTION_ADD" | "MESSAGE_REACTION_REMOVE" => {
            let r: WireReactionEvent = serde_json::from_value(d)?;
            let Some(user_id) = r.user_id else {
                return Ok(None);
            };
            let (channel_id, message_id, emoji) = (r.channel_id, r.message_id, r.emoji.into());
            Event::Reaction(if t == "MESSAGE_REACTION_ADD" {
                ReactionEvent::Add {
                    channel_id,
                    message_id,
                    user_id,
                    emoji,
                }
            } else {
                ReactionEvent::Remove {
                    channel_id,
                    message_id,
                    user_id,
                    emoji,
                }
            })
        }
        "MESSAGE_REACTION_REMOVE_ALL" => {
            let r: WireReactionEvent = serde_json::from_value(d)?;
            Event::Reaction(ReactionEvent::RemoveAll {
                channel_id: r.channel_id,
                message_id: r.message_id,
            })
        }
        "MESSAGE_REACTION_REMOVE_EMOJI" => {
            let r: WireReactionEvent = serde_json::from_value(d)?;
            Event::Reaction(ReactionEvent::RemoveEmoji {
                channel_id: r.channel_id,
                message_id: r.message_id,
                emoji: r.emoji.into(),
            })
        }
        "PRESENCE_UPDATE" => {
            let p: WirePresenceUpdate = serde_json::from_value(d)?;
            let Some(user_id) = p.user.and_then(|u| u.id) else {
                return Ok(None);
            };
            Event::PresenceUpdate {
                user_id,
                status: Presence::from_wire(&p.status),
                activities: super::cosmetics::parse_activities(&p.activities),
            }
        }
        "GUILD_ROLE_CREATE" | "GUILD_ROLE_UPDATE" => {
            let r: WireRoleEvent = serde_json::from_value(d)?;
            Event::RoleUpsert {
                guild_id: r.guild_id,
                role: r.role.into(),
            }
        }
        "GUILD_ROLE_DELETE" => {
            let r: WireRoleDelete = serde_json::from_value(d)?;
            Event::RoleDelete {
                guild_id: r.guild_id,
                role_id: r.role_id,
            }
        }
        "GUILD_MEMBER_UPDATE" => {
            let m: WireMemberUpdate = serde_json::from_value(d)?;
            let Some(u) = m.user else {
                return Ok(None);
            };
            Event::MemberRoles(vec![(m.guild_id, u.id, m.roles)])
        }
        "GUILD_MEMBER_LIST_UPDATE" => {
            let u: WireMemberListUpdate = serde_json::from_value(d)?;
            Event::MemberList(Box::new(u.into_domain()))
        }
        "GUILD_STICKERS_UPDATE" => {
            let e: WireStickersUpdate = serde_json::from_value(d)?;
            Event::GuildStickers {
                guild_id: e.guild_id,
                stickers: e.stickers.into_iter().map(Into::into).collect(),
            }
        }
        "GUILD_EMOJIS_UPDATE" => {
            let e: WireEmojisUpdate = serde_json::from_value(d)?;
            Event::GuildEmojis {
                guild_id: e.guild_id,
                emojis: e.emojis,
            }
        }
        "VOICE_STATE_UPDATE" => {
            let v: WireVoiceState = serde_json::from_value(d)?;
            Event::VoiceState(Box::new(v.into_domain(None)))
        }
        "VOICE_SERVER_UPDATE" => {
            let v: WireVoiceServer = serde_json::from_value(d)?;
            Event::VoiceServer(Box::new(VoiceServer {
                guild_id: v.guild_id,
                channel_id: v.channel_id,
                endpoint: v.endpoint.filter(|e| !e.is_empty() && e.len() < 256),
                token: v.token,
            }))
        }
        "CALL_CREATE" | "CALL_UPDATE" => {
            let c: WireCall = serde_json::from_value(d)?;
            Event::Call(Box::new(CallState {
                channel_id: c.channel_id,
                message_id: c.message_id,
                ringing: c.ringing.into_iter().take(100).collect(),
                voice_states: c
                    .voice_states
                    .into_iter()
                    .take(100)
                    .map(|v| v.into_domain(None))
                    .collect(),
                unavailable: c.unavailable,
            }))
        }
        "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" => {
            let r: WireRelationship = serde_json::from_value(d)?;
            match r.into_domain() {
                Some((rel, user)) => Event::RelationshipUpsert(Box::new(rel), user.map(Box::new)),
                None => return Ok(None),
            }
        }
        "RELATIONSHIP_REMOVE" => {
            let r: WireIdOnly = serde_json::from_value(d)?;
            Event::RelationshipRemove(r.id)
        }
        "CALL_DELETE" => {
            let c: WireIdChannel = serde_json::from_value(d)?;
            Event::CallDelete(c.channel_id)
        }
        _ => return Ok(None),
    }))
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireIdChannel {
    pub channel_id: Snowflake,
}

// ---------------------------------------------------------------------------
// REST bodies
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct MessageReference {
    pub message_id: MessageId,
    pub channel_id: ChannelId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guild_id: Option<GuildId>,
    pub fail_if_not_exists: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AllowedMentions {
    pub parse: Vec<&'static str>,
    pub replied_user: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateMessage<'a> {
    pub content: &'a str,
    pub nonce: &'a str,
    pub tts: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_reference: Option<MessageReference>,
    /// Only sent for replies with the ping toggled off, like the official
    /// client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_mentions: Option<AllowedMentions>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sticker_ids: Vec<Snowflake>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitBody {
    #[serde(default)]
    pub retry_after: f64,
    #[serde(default)]
    pub global: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcode_roundtrip() {
        for v in [0u8, 1, 2, 3, 4, 6, 7, 8, 9, 10, 11] {
            assert_eq!(OpCode::from_u8(v).map(|o| o as u8), Some(v));
        }
        assert!(OpCode::from_u8(5).is_none());
    }

    #[test]
    fn heartbeat_serialises_null_seq() {
        let p = heartbeat_payload(None);
        assert_eq!(serde_json::to_string(&p).unwrap(), r#"{"op":1,"d":null}"#);
        let p = heartbeat_payload(Some(5));
        assert_eq!(serde_json::to_string(&p).unwrap(), r#"{"op":1,"d":5}"#);
    }

    #[test]
    fn identify_contains_token_and_properties() {
        let p = identify_payload("tok", &ClientProperties::default(), Presence::Dnd, None);
        assert_eq!(p.d["presence"]["status"], "dnd");
        assert_eq!(p.op, 2);
        assert_eq!(p.d["token"], "tok");
        assert_eq!(p.d["properties"]["browser"], "Ferrocord");
    }

    #[test]
    fn unknown_dispatch_is_ignored() {
        assert!(
            parse_dispatch("TOTALLY_UNKNOWN", serde_json::json!({}))
                .unwrap()
                .is_none()
        );
    }
}
