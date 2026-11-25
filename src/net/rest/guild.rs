//! Server administration calls: invites, leaving, editing and deleting a
//! server, and creating, editing, deleting and reordering its channels.
//! Bodies and answers are built and parsed by the pure helpers at the
//! bottom (unit-tested).

use serde_json::{Value, json};

use super::{Method, RestClient};
use crate::auth::secrets::Secret;
use crate::error::{Error, Result};
use crate::net::mfa::{MFA_HEADER, MfaMethod};
use crate::net::model::*;

/// Invites made from the "Invite People" dialog last 7 days with no use
/// limit, the official client's default.
pub const INVITE_MAX_AGE_S: u32 = 7 * 24 * 60 * 60;

impl RestClient {
    /// `POST /channels/{c}/invites` — returns the invite code.
    pub async fn create_invite(&self, channel: ChannelId) -> Result<String> {
        let v = self
            .request(
                Method::POST,
                &format!("/channels/{channel}/invites"),
                Some(invite_body()),
            )
            .await?;
        parse_invite_code(&v).ok_or_else(|| Error::Protocol("invite answer without a code".into()))
    }

    /// `DELETE /users/@me/guilds/{g}` — leaves a server.
    pub async fn leave_guild(&self, guild: GuildId) -> Result<()> {
        self.request(
            Method::DELETE,
            &format!("/users/@me/guilds/{guild}"),
            Some(json!({ "lurking": false })),
        )
        .await
        .map(|_| ())
    }

    /// `PATCH /guilds/{g}` with the changed overview fields.
    pub async fn modify_guild(&self, guild: GuildId, edit: &GuildEdit) -> Result<()> {
        self.request(
            Method::PATCH,
            &format!("/guilds/{guild}"),
            Some(edit.body()),
        )
        .await
        .map(|_| ())
    }

    /// `POST /guilds/{g}/channels` — creates a channel or category.
    pub async fn create_channel(&self, guild: GuildId, new: &NewChannel) -> Result<Channel> {
        let v = self
            .request(
                Method::POST,
                &format!("/guilds/{guild}/channels"),
                Some(new.body()),
            )
            .await?;
        let w: crate::net::types::WireChannel = serde_json::from_value(v)
            .map_err(|_| Error::Protocol("channel answer without a channel".into()))?;
        Ok(w.into_domain(Some(guild)).0)
    }

    /// `PATCH /channels/{c}` with the changed fields.
    pub async fn modify_channel(&self, channel: ChannelId, edit: &ChannelEdit) -> Result<()> {
        self.request(
            Method::PATCH,
            &format!("/channels/{channel}"),
            Some(edit.body()),
        )
        .await
        .map(|_| ())
    }

    /// `DELETE /channels/{c}` — deletes a server channel or category.
    pub async fn delete_channel(&self, channel: ChannelId) -> Result<()> {
        self.request(Method::DELETE, &format!("/channels/{channel}"), None)
            .await
            .map(|_| ())
    }

    /// `PATCH /guilds/{g}/channels` — new positions of several channels.
    pub async fn reorder_channels(&self, guild: GuildId, moves: &[(ChannelId, i32)]) -> Result<()> {
        self.request(
            Method::PATCH,
            &format!("/guilds/{guild}/channels"),
            Some(reorder_body(moves)),
        )
        .await
        .map(|_| ())
    }

    /// `POST /guilds/{g}/roles` — creates a role; returns it.
    pub async fn create_role(&self, guild: GuildId, edit: &RoleEdit) -> Result<Role> {
        let v = self
            .request(
                Method::POST,
                &format!("/guilds/{guild}/roles"),
                Some(edit.body()),
            )
            .await?;
        let w: crate::net::types::WireRole = serde_json::from_value(v)
            .map_err(|_| Error::Protocol("role answer without a role".into()))?;
        Ok(w.into())
    }

    /// `PATCH /guilds/{g}/roles/{r}` with the changed fields.
    pub async fn modify_role(&self, guild: GuildId, role: RoleId, edit: &RoleEdit) -> Result<()> {
        self.request(
            Method::PATCH,
            &format!("/guilds/{guild}/roles/{role}"),
            Some(edit.body()),
        )
        .await
        .map(|_| ())
    }

    /// `DELETE /guilds/{g}/roles/{r}`.
    pub async fn delete_role(&self, guild: GuildId, role: RoleId) -> Result<()> {
        self.request(
            Method::DELETE,
            &format!("/guilds/{guild}/roles/{role}"),
            None,
        )
        .await
        .map(|_| ())
    }

    /// `PUT` / `DELETE /guilds/{g}/members/{u}/roles/{r}` — gives or takes
    /// a role.
    pub async fn set_member_role(
        &self,
        guild: GuildId,
        user: UserId,
        role: RoleId,
        on: bool,
    ) -> Result<()> {
        let method = if on { Method::PUT } else { Method::DELETE };
        self.request(
            method,
            &format!("/guilds/{guild}/members/{user}/roles/{role}"),
            None,
        )
        .await
        .map(|_| ())
    }

    /// Kick, ban or timeout, with the optional audit-log reason.
    pub async fn moderate(&self, guild: GuildId, user: UserId, action: &ModAction) -> Result<()> {
        let (method, path, body) = action.request(guild, user, crate::mock::now_ms());
        let reason = action.reason().map(percent_encode);
        self.send_with(method, &path, |req| {
            let req = match &reason {
                Some(r) => req.header("X-Audit-Log-Reason", r.as_str()),
                None => req,
            };
            match &body {
                Some(b) => req.json(b),
                None => req,
            }
        })
        .await
        .map(|_| ())
    }

    /// `DELETE /guilds/{g}` — deletes a server (owner only). With 2FA on,
    /// the first try fails with [`Error::MfaRequired`]; the retry carries
    /// the token of [`mfa_finish`](Self::mfa_finish).
    pub async fn delete_guild(&self, guild: GuildId, mfa: Option<&Secret>) -> Result<()> {
        self.send_with(
            Method::DELETE,
            &format!("/guilds/{guild}"),
            |req| match mfa {
                Some(t) => req.header(MFA_HEADER, t.expose()),
                None => req,
            },
        )
        .await
        .map(|_| ())
    }

    /// `POST /mfa/finish {ticket, mfa_type, data}` — answers a 2FA demand;
    /// returns the token for the repeated request.
    pub async fn mfa_finish(
        &self,
        ticket: &Secret,
        method: MfaMethod,
        code: &str,
    ) -> Result<Secret> {
        let v = self
            .request(
                Method::POST,
                "/mfa/finish",
                Some(json!({ "ticket": ticket.expose(), "mfa_type": method.wire(), "data": code })),
            )
            .await
            .map_err(|e| match e {
                // A wrong code is a 400 (or 401) "Invalid two-factor code".
                Error::Http {
                    status: 400 | 401, ..
                } => Error::Auth("That code did not work. Check it and try again.".into()),
                other => other,
            })?;
        v.get("token")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(Secret::new)
            .ok_or_else(|| Error::Protocol("2FA answer without a token".into()))
    }

    /// `PATCH /guilds/{g}/roles [{id, position}]` — new role positions.
    pub async fn reorder_roles(&self, guild: GuildId, moves: &[(RoleId, i32)]) -> Result<()> {
        self.request(
            Method::PATCH,
            &format!("/guilds/{guild}/roles"),
            Some(reorder_body(moves)),
        )
        .await
        .map(|_| ())
    }

    /// `PUT /channels/{c}/permissions/{id} {type, allow, deny}` — creates or
    /// replaces a channel permission overwrite.
    pub async fn set_overwrite(&self, channel: ChannelId, o: &PermissionOverwrite) -> Result<()> {
        self.request(
            Method::PUT,
            &format!("/channels/{channel}/permissions/{}", o.id),
            Some(overwrite_body(o)),
        )
        .await
        .map(|_| ())
    }

    /// `DELETE /channels/{c}/permissions/{id}` — removes an overwrite.
    pub async fn delete_overwrite(&self, channel: ChannelId, id: Snowflake) -> Result<()> {
        self.request(
            Method::DELETE,
            &format!("/channels/{channel}/permissions/{id}"),
            None,
        )
        .await
        .map(|_| ())
    }

    /// `GET /guilds/{g}/bans?limit=1000[&after=…]` — every ban, paged
    /// (at most [`BANS_MAX`]).
    pub async fn bans(&self, guild: GuildId) -> Result<Vec<Ban>> {
        let mut out: Vec<Ban> = Vec::new();
        loop {
            let mut path = format!("/guilds/{guild}/bans?limit={BANS_PAGE}");
            if let Some(last) = out.last() {
                path.push_str(&format!("&after={}", last.user.id));
            }
            let v = self.request(Method::GET, &path, None).await?;
            let page = parse_bans(&v);
            let full = page.len() >= BANS_PAGE;
            out.extend(page);
            if !full || out.len() >= BANS_MAX {
                break;
            }
        }
        out.truncate(BANS_MAX);
        Ok(out)
    }

    /// `DELETE /guilds/{g}/bans/{u}` — revokes a ban.
    pub async fn unban(&self, guild: GuildId, user: UserId) -> Result<()> {
        self.request(
            Method::DELETE,
            &format!("/guilds/{guild}/bans/{user}"),
            None,
        )
        .await
        .map(|_| ())
    }
}

/// Bans asked for per page (Discord's maximum).
pub const BANS_PAGE: usize = 1000;
/// Bans loaded at most.
pub const BANS_MAX: usize = 10_000;

/// A banned user.
#[derive(Clone, PartialEq)]
pub struct Ban {
    pub user: User,
    pub reason: Option<String>,
}

impl std::fmt::Debug for Ban {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ban")
            .field("user", &self.user.id)
            .field("reason", &self.reason.as_ref().map(|_| "<set>"))
            .finish()
    }
}

/// `GET /guilds/{g}/bans` answer; entries without a user are skipped.
pub fn parse_bans(v: &Value) -> Vec<Ban> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| {
            let user: crate::net::types::WireUser =
                serde_json::from_value(b.get("user")?.clone()).ok()?;
            Some(Ban {
                user: user.into(),
                reason: b
                    .get("reason")
                    .and_then(Value::as_str)
                    .filter(|r| !r.trim().is_empty())
                    .map(str::to_owned),
            })
        })
        .collect()
}

/// Body of `PUT /channels/{c}/permissions/{id}`: bits as decimal strings.
pub fn overwrite_body(o: &PermissionOverwrite) -> Value {
    json!({
        "type": u8::from(o.member),
        "allow": o.allow.to_string(),
        "deny": o.deny.to_string(),
    })
}

/// Body of `POST /channels/{c}/invites`.
pub fn invite_body() -> Value {
    json!({
        "max_age": INVITE_MAX_AGE_S,
        "max_uses": 0,
        "temporary": false,
    })
}

pub fn parse_invite_code(v: &Value) -> Option<String> {
    v.get("code")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
}

/// Changes to a server's overview. `None` = unchanged.
#[derive(Clone, Default, PartialEq)]
pub struct GuildEdit {
    pub name: Option<String>,
    /// `Some(None)` removes the icon, `Some(Some(uri))` sets a `data:` URI.
    pub icon: Option<Option<String>>,
}

impl std::fmt::Debug for GuildEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuildEdit")
            .field("name", &self.name.as_ref().map(|_| "<set>"))
            .field(
                "icon",
                &self
                    .icon
                    .as_ref()
                    .map(|i| if i.is_some() { "<set>" } else { "<removed>" }),
            )
            .finish()
    }
}

impl GuildEdit {
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.icon.is_none()
    }

    pub fn body(&self) -> Value {
        let mut m = serde_json::Map::new();
        if let Some(n) = &self.name {
            m.insert("name".into(), json!(n.trim()));
        }
        if let Some(i) = &self.icon {
            m.insert("icon".into(), json!(i));
        }
        Value::Object(m)
    }
}

/// Kinds a channel can be created as from Ferrocord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewKind {
    Text,
    Voice,
    Category,
}

impl NewKind {
    pub fn wire(self) -> u8 {
        match self {
            Self::Text => 0,
            Self::Voice => 2,
            Self::Category => 4,
        }
    }
}

/// Body of `POST /guilds/{g}/channels`.
#[derive(Clone, PartialEq)]
pub struct NewChannel {
    pub name: String,
    pub kind: NewKind,
    pub parent: Option<ChannelId>,
}

impl std::fmt::Debug for NewChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewChannel")
            .field("kind", &self.kind)
            .field("parent", &self.parent)
            .finish_non_exhaustive()
    }
}

impl NewChannel {
    pub fn body(&self) -> Value {
        let mut b = json!({
            "name": self.name.trim(),
            "type": self.kind.wire(),
            "permission_overwrites": [],
        });
        if self.kind != NewKind::Category
            && let Some(p) = self.parent
        {
            b["parent_id"] = json!(p.to_string());
        }
        b
    }
}

/// Changes to a channel. `None` = unchanged.
#[derive(Clone, Default, PartialEq)]
pub struct ChannelEdit {
    pub name: Option<String>,
    /// `Some("")` clears the topic.
    pub topic: Option<String>,
    pub nsfw: Option<bool>,
    /// `Some(None)` moves the channel out of any category.
    pub parent: Option<Option<ChannelId>>,
}

impl std::fmt::Debug for ChannelEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelEdit")
            .field("name", &self.name.as_ref().map(|_| "<set>"))
            .field("topic", &self.topic.as_ref().map(|_| "<set>"))
            .field("nsfw", &self.nsfw)
            .field("parent", &self.parent)
            .finish()
    }
}

impl ChannelEdit {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn body(&self) -> Value {
        let mut m = serde_json::Map::new();
        if let Some(n) = &self.name {
            m.insert("name".into(), json!(n.trim()));
        }
        if let Some(t) = &self.topic {
            let t = t.trim();
            m.insert(
                "topic".into(),
                if t.is_empty() { Value::Null } else { json!(t) },
            );
        }
        if let Some(n) = self.nsfw {
            m.insert("nsfw".into(), json!(n));
        }
        if let Some(p) = self.parent {
            m.insert("parent_id".into(), json!(p.map(|p| p.to_string())));
        }
        Value::Object(m)
    }
}

/// Body of `PATCH /guilds/{g}/channels`.
pub fn reorder_body(moves: &[(Snowflake, i32)]) -> Value {
    Value::Array(
        moves
            .iter()
            .map(|(id, pos)| json!({ "id": id.to_string(), "position": pos }))
            .collect(),
    )
}

/// How Discord stores a text channel name: lower case, runs of spaces as
/// one dash. Voice channels and categories keep the name as typed.
pub fn channel_slug(name: &str) -> String {
    let mut out = String::new();
    for w in name.split_whitespace() {
        if !out.is_empty() {
            out.push('-');
        }
        out.extend(w.chars().flat_map(char::to_lowercase));
    }
    out
}

/// Changes to a role (or a new role). `None` = unchanged / default.
#[derive(Clone, Default, PartialEq)]
pub struct RoleEdit {
    pub name: Option<String>,
    pub color: Option<u32>,
    pub permissions: Option<u64>,
    pub hoist: Option<bool>,
    pub mentionable: Option<bool>,
}

impl std::fmt::Debug for RoleEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoleEdit")
            .field("name", &self.name.as_ref().map(|_| "<set>"))
            .field("color", &self.color)
            .field("permissions", &self.permissions)
            .field("hoist", &self.hoist)
            .field("mentionable", &self.mentionable)
            .finish()
    }
}

impl RoleEdit {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Permission bits go as a decimal string, like the official client.
    pub fn body(&self) -> Value {
        let mut m = serde_json::Map::new();
        if let Some(n) = &self.name {
            m.insert("name".into(), json!(n.trim()));
        }
        if let Some(c) = self.color {
            m.insert("color".into(), json!(c));
        }
        if let Some(p) = self.permissions {
            m.insert("permissions".into(), json!(p.to_string()));
        }
        if let Some(h) = self.hoist {
            m.insert("hoist".into(), json!(h));
        }
        if let Some(x) = self.mentionable {
            m.insert("mentionable".into(), json!(x));
        }
        Value::Object(m)
    }
}

/// Timeout lengths offered, in seconds (Discord allows up to 28 days).
pub const TIMEOUTS: [(u64, &str); 6] = [
    (60, "60 seconds"),
    (5 * 60, "5 minutes"),
    (10 * 60, "10 minutes"),
    (60 * 60, "1 hour"),
    (24 * 60 * 60, "1 day"),
    (7 * 24 * 60 * 60, "1 week"),
];

/// How much message history a ban deletes, in seconds (7 days at most).
pub const BAN_HISTORY: [(u32, &str); 4] = [
    (0, "Don't delete any"),
    (60 * 60, "Previous hour"),
    (24 * 60 * 60, "Previous 24 hours"),
    (7 * 24 * 60 * 60, "Previous 7 days"),
];

/// A moderation action on a member.
#[derive(Clone, PartialEq)]
pub enum ModAction {
    Kick {
        reason: String,
    },
    Ban {
        delete_seconds: u32,
        reason: String,
    },
    /// `None` removes the timeout.
    Timeout {
        seconds: Option<u64>,
        reason: String,
    },
}

impl std::fmt::Debug for ModAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kick { .. } => f.write_str("Kick"),
            Self::Ban { delete_seconds, .. } => write!(f, "Ban({delete_seconds}s)"),
            Self::Timeout { seconds, .. } => write!(f, "Timeout({seconds:?})"),
        }
    }
}

impl ModAction {
    pub fn reason(&self) -> Option<&str> {
        let r = match self {
            Self::Kick { reason } | Self::Ban { reason, .. } | Self::Timeout { reason, .. } => {
                reason.trim()
            }
        };
        (!r.is_empty()).then_some(r)
    }

    /// Method, path and body; `now_ms` dates the end of a timeout.
    pub fn request(
        &self,
        guild: GuildId,
        user: UserId,
        now_ms: u64,
    ) -> (Method, String, Option<Value>) {
        match self {
            Self::Kick { .. } => (
                Method::DELETE,
                format!("/guilds/{guild}/members/{user}"),
                None,
            ),
            Self::Ban { delete_seconds, .. } => (
                Method::PUT,
                format!("/guilds/{guild}/bans/{user}"),
                Some(json!({ "delete_message_seconds": (*delete_seconds).min(604_800) })),
            ),
            Self::Timeout { seconds, .. } => (
                Method::PATCH,
                format!("/guilds/{guild}/members/{user}"),
                Some(json!({
                    "communication_disabled_until":
                        seconds.map(|s| iso_utc(now_ms.saturating_add(s.saturating_mul(1000)))),
                })),
            ),
        }
    }
}

/// `2026-11-25T10:00:00Z` for a Unix time in milliseconds.
pub fn iso_utc(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

/// Public link of an invite code.
pub fn invite_url(code: &str) -> String {
    format!("https://discord.gg/{code}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_body_and_code() {
        let b = invite_body();
        assert_eq!(b["max_age"], 604_800);
        assert_eq!(b["max_uses"], 0);
        assert_eq!(b["temporary"], false);
        assert_eq!(
            parse_invite_code(&json!({"code": "abc", "guild": {}})).as_deref(),
            Some("abc")
        );
        assert_eq!(parse_invite_code(&json!({"code": ""})), None);
        assert_eq!(parse_invite_code(&json!({})), None);
        assert_eq!(invite_url("abc"), "https://discord.gg/abc");
    }

    #[test]
    fn overwrite_and_ban_bodies() {
        let o = PermissionOverwrite {
            id: Snowflake(5),
            member: true,
            allow: 1024,
            deny: 2048,
        };
        assert_eq!(
            overwrite_body(&o),
            json!({"type": 1, "allow": "1024", "deny": "2048"})
        );
        let v = json!([
            {"user": {"id": "9", "username": "spam"}, "reason": "ads"},
            {"user": {"id": "8", "username": "x"}, "reason": "  "},
            {"reason": "no user"}
        ]);
        let bans = parse_bans(&v);
        assert_eq!(bans.len(), 2);
        assert_eq!(bans[0].reason.as_deref(), Some("ads"));
        assert_eq!(bans[1].reason, None);
        assert!(!format!("{:?}", bans[0]).contains("ads"));
        assert_eq!(
            reorder_body(&[(Snowflake(3), 2)]),
            json!([{"id": "3", "position": 2}])
        );
    }

    #[test]
    fn role_and_moderation_requests() {
        let e = RoleEdit {
            name: Some(" Mods ".into()),
            color: Some(0xe91e63),
            permissions: Some(1 << 40 | 2),
            hoist: None,
            mentionable: Some(true),
        };
        assert_eq!(
            e.body(),
            json!({"name": "Mods", "color": 0xe91e63, "permissions": "1099511627778", "mentionable": true})
        );
        assert_eq!(RoleEdit::default().body(), json!({}));
        assert!(!format!("{e:?}").contains("Mods"));
        let (g, u) = (Snowflake(10), Snowflake(20));
        let kick = ModAction::Kick {
            reason: "  ".into(),
        };
        assert_eq!(kick.reason(), None);
        let (m, p, b) = kick.request(g, u, 0);
        assert_eq!(
            (m, p.as_str(), b),
            (Method::DELETE, "/guilds/10/members/20", None)
        );
        let ban = ModAction::Ban {
            delete_seconds: 9_999_999,
            reason: "spam".into(),
        };
        assert_eq!(ban.reason(), Some("spam"));
        let (m, p, b) = ban.request(g, u, 0);
        assert_eq!((m, p.as_str()), (Method::PUT, "/guilds/10/bans/20"));
        assert_eq!(b, Some(json!({"delete_message_seconds": 604_800})));
        let t = ModAction::Timeout {
            seconds: Some(60),
            reason: String::new(),
        };
        let (m, p, b) = t.request(g, u, 1_764_064_800_000);
        assert_eq!((m, p.as_str()), (Method::PATCH, "/guilds/10/members/20"));
        assert_eq!(
            b,
            Some(json!({"communication_disabled_until": "2025-11-25T10:01:00Z"}))
        );
        let t = ModAction::Timeout {
            seconds: None,
            reason: String::new(),
        };
        assert_eq!(
            t.request(g, u, 0).2,
            Some(json!({"communication_disabled_until": null}))
        );
        assert!(!format!("{ban:?}").contains("spam"));
    }

    #[test]
    fn channel_bodies() {
        let n = NewChannel {
            name: " general ".into(),
            kind: NewKind::Text,
            parent: Some(Snowflake(9)),
        };
        assert_eq!(
            n.body(),
            json!({"name": "general", "type": 0, "parent_id": "9", "permission_overwrites": []})
        );
        let n = NewChannel {
            name: "Stuff".into(),
            kind: NewKind::Category,
            parent: Some(Snowflake(9)),
        };
        assert_eq!(
            n.body(),
            json!({"name": "Stuff", "type": 4, "permission_overwrites": []})
        );
        assert!(!format!("{n:?}").contains("Stuff"));
        assert_eq!(ChannelEdit::default().body(), json!({}));
        let e = ChannelEdit {
            name: Some("a".into()),
            topic: Some("  ".into()),
            nsfw: Some(true),
            parent: Some(None),
        };
        assert_eq!(
            e.body(),
            json!({"name": "a", "topic": null, "nsfw": true, "parent_id": null})
        );
        let e = ChannelEdit {
            parent: Some(Some(Snowflake(4))),
            ..Default::default()
        };
        assert_eq!(e.body(), json!({"parent_id": "4"}));
        assert_eq!(
            reorder_body(&[(Snowflake(1), 0), (Snowflake(2), 1)]),
            json!([{"id": "1", "position": 0}, {"id": "2", "position": 1}])
        );
        assert_eq!(channel_slug("  Off  Topic Chat "), "off-topic-chat");
        assert_eq!(channel_slug("ÁRBOL"), "árbol");
    }

    #[test]
    fn guild_edit_sends_only_changes() {
        assert!(GuildEdit::default().is_empty());
        assert_eq!(GuildEdit::default().body(), json!({}));
        let e = GuildEdit {
            name: Some("  New  ".into()),
            icon: None,
        };
        assert_eq!(e.body(), json!({"name": "New"}));
        let e = GuildEdit {
            name: None,
            icon: Some(None),
        };
        assert_eq!(e.body(), json!({"icon": null}));
        let e = GuildEdit {
            name: None,
            icon: Some(Some("data:image/png;base64,AA".into())),
        };
        assert_eq!(e.body(), json!({"icon": "data:image/png;base64,AA"}));
        assert!(!format!("{e:?}").contains("base64"));
    }
}
