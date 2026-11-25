//! Discord permission maths: server-wide bits from the roles, then the
//! channel overwrites (`@everyone`, roles, member), like the official
//! client. Pure functions, unit-tested.

use crate::net::model::*;

pub const CREATE_INSTANT_INVITE: u64 = 1 << 0;
pub const KICK_MEMBERS: u64 = 1 << 1;
pub const BAN_MEMBERS: u64 = 1 << 2;
pub const ADMINISTRATOR: u64 = 1 << 3;
pub const MANAGE_CHANNELS: u64 = 1 << 4;
pub const MANAGE_GUILD: u64 = 1 << 5;
pub const VIEW_CHANNEL: u64 = 1 << 10;
pub const SEND_MESSAGES: u64 = 1 << 11;
pub const MANAGE_MESSAGES: u64 = 1 << 13;
pub const MANAGE_ROLES: u64 = 1 << 28;
pub const MODERATE_MEMBERS: u64 = 1 << 40;

/// One editable permission: (bit, name, description).
pub type PermissionInfo = (u64, &'static str, &'static str);

/// The permissions the role editor offers, grouped like the official
/// client: (section, permissions).
pub const EDITABLE: [(&str, &[PermissionInfo]); 5] = [
    (
        "General Server Permissions",
        &[
            (
                VIEW_CHANNEL,
                "View Channels",
                "Members can view channels by default (excluding private channels).",
            ),
            (
                MANAGE_CHANNELS,
                "Manage Channels",
                "Create, edit and delete channels.",
            ),
            (
                MANAGE_ROLES,
                "Manage Roles",
                "Create roles and edit or delete roles lower than their highest role.",
            ),
            (
                1 << 30,
                "Manage Expressions",
                "Add, edit and remove custom emoji and stickers.",
            ),
            (
                1 << 7,
                "View Audit Log",
                "View a record of who made which changes in this server.",
            ),
            (
                MANAGE_GUILD,
                "Manage Server",
                "Change the server's name and icon, and view all invites.",
            ),
        ],
    ),
    (
        "Membership Permissions",
        &[
            (
                CREATE_INSTANT_INVITE,
                "Create Invite",
                "Invite new people to this server.",
            ),
            (1 << 26, "Change Nickname", "Change their own nickname."),
            (
                1 << 27,
                "Manage Nicknames",
                "Change the nicknames of other members.",
            ),
            (
                KICK_MEMBERS,
                "Kick Members",
                "Remove other members from this server.",
            ),
            (
                BAN_MEMBERS,
                "Ban Members",
                "Permanently ban and delete the message history of other members.",
            ),
            (
                MODERATE_MEMBERS,
                "Timeout Members",
                "Stop users from chatting, reacting or speaking for a while.",
            ),
        ],
    ),
    (
        "Text Channel Permissions",
        &[
            (
                SEND_MESSAGES,
                "Send Messages",
                "Send messages in text channels.",
            ),
            (
                1 << 14,
                "Embed Links",
                "Links shared show embedded content.",
            ),
            (1 << 15, "Attach Files", "Upload files and media."),
            (
                1 << 6,
                "Add Reactions",
                "Add new emoji reactions to messages.",
            ),
            (
                1 << 18,
                "Use External Emoji",
                "Use emoji from other servers (with Nitro).",
            ),
            (
                1 << 17,
                "Mention @everyone, @here, and All Roles",
                "Notify everyone in a channel or any role.",
            ),
            (
                MANAGE_MESSAGES,
                "Manage Messages",
                "Delete and pin messages from other members.",
            ),
            (
                1 << 16,
                "Read Message History",
                "Read messages sent before they joined.",
            ),
        ],
    ),
    (
        "Voice Channel Permissions",
        &[
            (1 << 20, "Connect", "Join voice channels."),
            (1 << 21, "Speak", "Talk in voice channels."),
            (1 << 9, "Video", "Share their camera or screen."),
            (
                1 << 22,
                "Mute Members",
                "Mute other members in voice channels.",
            ),
            (
                1 << 23,
                "Deafen Members",
                "Deafen other members in voice channels.",
            ),
            (
                1 << 24,
                "Move Members",
                "Move or disconnect members between voice channels.",
            ),
        ],
    ),
    (
        "Advanced Permissions",
        &[(
            ADMINISTRATOR,
            "Administrator",
            "Every permission and bypasses channel overwrites. Dangerous to grant.",
        )],
    ),
];

/// The permissions a channel's overwrites can change, by channel kind:
/// General for every channel, Text for text channels, Voice for voice
/// channels, all three for a category.
pub const CHANNEL_GENERAL: &[PermissionInfo] = &[
    (
        VIEW_CHANNEL,
        "View Channel",
        "Members can see this channel.",
    ),
    (
        MANAGE_CHANNELS,
        "Manage Channel",
        "Change the name and topic, or delete the channel.",
    ),
    (
        MANAGE_ROLES,
        "Manage Permissions",
        "Change this channel's permissions.",
    ),
    (
        CREATE_INSTANT_INVITE,
        "Create Invite",
        "Invite new people through this channel.",
    ),
];
pub const CHANNEL_TEXT: &[PermissionInfo] = &[
    (
        SEND_MESSAGES,
        "Send Messages",
        "Send messages in this channel.",
    ),
    (
        1 << 14,
        "Embed Links",
        "Links shared show embedded content.",
    ),
    (1 << 15, "Attach Files", "Upload files and media."),
    (
        1 << 6,
        "Add Reactions",
        "Add new emoji reactions to messages.",
    ),
    (
        1 << 18,
        "Use External Emoji",
        "Use emoji from other servers (with Nitro).",
    ),
    (
        1 << 17,
        "Mention @everyone, @here, and All Roles",
        "Notify everyone in this channel or any role.",
    ),
    (
        MANAGE_MESSAGES,
        "Manage Messages",
        "Delete and pin messages from other members.",
    ),
    (
        1 << 16,
        "Read Message History",
        "Read messages sent before they could see the channel.",
    ),
];
pub const CHANNEL_VOICE: &[PermissionInfo] = &[
    (1 << 20, "Connect", "Join this voice channel."),
    (1 << 21, "Speak", "Talk in this voice channel."),
    (1 << 9, "Video", "Share their camera or screen."),
    (1 << 22, "Mute Members", "Mute other members here."),
    (1 << 23, "Deafen Members", "Deafen other members here."),
    (1 << 24, "Move Members", "Move or disconnect members here."),
];

/// The overwrite sections shown for a channel of `kind`.
pub fn channel_sections(kind: ChannelKind) -> Vec<(&'static str, &'static [PermissionInfo])> {
    let general = ("General Channel Permissions", CHANNEL_GENERAL);
    let text = ("Text Channel Permissions", CHANNEL_TEXT);
    let voice = ("Voice Channel Permissions", CHANNEL_VOICE);
    match kind {
        ChannelKind::Category => vec![general, text, voice],
        ChannelKind::Voice | ChannelKind::Stage => vec![general, voice],
        _ => vec![general, text],
    }
}

/// Highest role position of a member: `None` for the owner (above every
/// role), else the top position among `roles` (0 without any).
pub fn top_position(guild: &Guild, user: UserId, roles: &[RoleId]) -> Option<i32> {
    if guild.owner_id == Some(user) {
        return None;
    }
    Some(
        guild
            .roles
            .iter()
            .filter(|r| r.id != guild.id && roles.contains(&r.id))
            .map(|r| r.position)
            .max()
            .unwrap_or(0),
    )
}

/// Whether a member at `actor` (from [`top_position`]) outranks one at
/// `target`: the owner outranks everyone, nobody outranks the owner.
pub fn outranks(actor: Option<i32>, target: Option<i32>) -> bool {
    match (actor, target) {
        (None, Some(_)) => true,
        (_, None) => false,
        (Some(a), Some(t)) => a > t,
    }
}
pub const ALL: u64 = u64::MAX;

/// Server-wide permissions of `me` (with `my_roles`) in `guild`: the
/// owner and Administrator get everything, everyone else the union of
/// `@everyone` (the role whose id is the guild id) and their roles.
pub fn base(guild: &Guild, me: Option<UserId>, my_roles: &[RoleId]) -> u64 {
    if me.is_some() && guild.owner_id == me {
        return ALL;
    }
    let bits = guild
        .roles
        .iter()
        .filter(|r| r.id == guild.id || my_roles.contains(&r.id))
        .fold(0, |acc, r| acc | r.permissions);
    if bits & ADMINISTRATOR != 0 { ALL } else { bits }
}

/// Applies a channel's overwrites to `base`: `@everyone` first, then all
/// of the member's roles together, then the member's own overwrite.
pub fn in_channel(
    base: u64,
    guild: GuildId,
    me: Option<UserId>,
    my_roles: &[RoleId],
    overwrites: &[PermissionOverwrite],
) -> u64 {
    if base & ADMINISTRATOR != 0 {
        return ALL;
    }
    let mut bits = base;
    if let Some(o) = overwrites.iter().find(|o| !o.member && o.id == guild) {
        bits = (bits & !o.deny) | o.allow;
    }
    let (allow, deny) = overwrites
        .iter()
        .filter(|o| !o.member && o.id != guild && my_roles.contains(&o.id))
        .fold((0, 0), |(a, d), o| (a | o.allow, d | o.deny));
    bits = (bits & !deny) | allow;
    if let Some(me) = me
        && let Some(o) = overwrites.iter().find(|o| o.member && o.id == me)
    {
        bits = (bits & !o.deny) | o.allow;
    }
    bits
}

/// Whether the permissions of `guild` can be worked out: the role list
/// holds `@everyone`. Partial guilds (mock data, a server that is still
/// loading) have none, and are then treated as fully visible.
pub fn known(guild: &Guild) -> bool {
    guild.roles.iter().any(|r| r.id == guild.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: GuildId = Snowflake(10);
    const ME: UserId = Snowflake(1);
    const MOD: RoleId = Snowflake(20);
    const MUTED: RoleId = Snowflake(21);

    fn role(id: RoleId, permissions: u64) -> Role {
        Role {
            id,
            name: String::new(),
            color: 0,
            position: 0,
            permissions,
            hoist: false,
            mentionable: false,
        }
    }

    fn guild(owner: Option<UserId>, roles: Vec<Role>) -> Guild {
        Guild {
            id: G,
            name: "g".into(),
            icon: None,
            roles,
            owner_id: owner,
            emojis: vec![],
            stickers: vec![],
            premium_tier: 0,
        }
    }

    fn ow(id: Snowflake, member: bool, allow: u64, deny: u64) -> PermissionOverwrite {
        PermissionOverwrite {
            id,
            member,
            allow,
            deny,
        }
    }

    #[test]
    fn owner_and_admin_get_everything() {
        let g = guild(Some(ME), vec![role(G, VIEW_CHANNEL)]);
        assert_eq!(base(&g, Some(ME), &[]), ALL);
        let g = guild(None, vec![role(G, 0), role(MOD, ADMINISTRATOR)]);
        assert_eq!(base(&g, Some(ME), &[MOD]), ALL);
        assert_eq!(base(&g, Some(ME), &[]), 0);
        // An admin ignores channel overwrites.
        let deny_all = [ow(G, false, 0, VIEW_CHANNEL)];
        assert_eq!(in_channel(ALL, G, Some(ME), &[MOD], &deny_all), ALL);
    }

    #[test]
    fn unknown_owner_is_not_the_owner() {
        let g = guild(None, vec![role(G, VIEW_CHANNEL)]);
        assert_eq!(base(&g, None, &[]), VIEW_CHANNEL);
    }

    #[test]
    fn roles_add_up() {
        let g = guild(
            None,
            vec![
                role(G, VIEW_CHANNEL),
                role(MOD, MANAGE_MESSAGES | KICK_MEMBERS),
            ],
        );
        let b = base(&g, Some(ME), &[MOD]);
        assert_eq!(b, VIEW_CHANNEL | MANAGE_MESSAGES | KICK_MEMBERS);
    }

    #[test]
    fn overwrite_order_everyone_roles_member() {
        let b = VIEW_CHANNEL | SEND_MESSAGES;
        // Hidden from @everyone, shown to moderators.
        let o = [
            ow(G, false, 0, VIEW_CHANNEL),
            ow(MOD, false, VIEW_CHANNEL, 0),
        ];
        assert_eq!(in_channel(b, G, Some(ME), &[], &o) & VIEW_CHANNEL, 0);
        assert_ne!(in_channel(b, G, Some(ME), &[MOD], &o) & VIEW_CHANNEL, 0);
        // A role allow beats another role's deny (allows are applied last).
        let o = [
            ow(MUTED, false, 0, SEND_MESSAGES),
            ow(MOD, false, SEND_MESSAGES, 0),
        ];
        assert_ne!(
            in_channel(b, G, Some(ME), &[MOD, MUTED], &o) & SEND_MESSAGES,
            0
        );
        // The member overwrite has the last word.
        let o = [
            ow(MOD, false, SEND_MESSAGES, 0),
            ow(ME, true, 0, SEND_MESSAGES),
        ];
        assert_eq!(in_channel(b, G, Some(ME), &[MOD], &o) & SEND_MESSAGES, 0);
        // Overwrites of other roles do not apply.
        let o = [ow(MUTED, false, 0, SEND_MESSAGES)];
        assert_eq!(in_channel(b, G, Some(ME), &[MOD], &o), b);
    }

    #[test]
    fn hierarchy() {
        let mut lo = role(MOD, 0);
        lo.position = 1;
        let mut hi = role(MUTED, 0);
        hi.position = 5;
        let g = guild(Some(Snowflake(99)), vec![role(G, 0), lo, hi]);
        assert_eq!(top_position(&g, Snowflake(99), &[]), None);
        assert_eq!(top_position(&g, ME, &[]), Some(0));
        assert_eq!(top_position(&g, ME, &[MOD, MUTED]), Some(5));
        assert!(outranks(None, Some(100)));
        assert!(!outranks(Some(100), None));
        assert!(outranks(Some(5), Some(1)));
        assert!(!outranks(Some(1), Some(1)));
        // Every editable bit is listed once.
        let mut seen = 0u64;
        for (_, list) in EDITABLE {
            for (bit, _, _) in list {
                assert_eq!(seen & bit, 0);
                seen |= bit;
            }
        }
    }

    #[test]
    fn known_needs_everyone() {
        assert!(!known(&guild(None, vec![])));
        assert!(!known(&guild(None, vec![role(MOD, 0)])));
        assert!(known(&guild(None, vec![role(G, 0)])));
    }
}
