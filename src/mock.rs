//! Mock data used by tests and `--mock` screenshot mode.
//!
//! Every name, server and message here is invented.

use crate::net::model::*;

pub const ME: UserId = Snowflake(1);

pub const GUILD_RUST: GuildId = Snowflake(100);
pub const GUILD_ICED: GuildId = Snowflake(200);
pub const GUILD_LINUX: GuildId = Snowflake(300);
pub const GUILD_GAMES: GuildId = Snowflake(400);
pub const GUILD_CAFE: GuildId = Snowflake(500);

pub const CAT_INFO: ChannelId = Snowflake(110);
pub const CAT_TEXT: ChannelId = Snowflake(111);
pub const CAT_VOICE: ChannelId = Snowflake(112);
pub const CH_WELCOME: ChannelId = Snowflake(120);
pub const CH_ANNOUNCEMENTS: ChannelId = Snowflake(121);
pub const CH_RULES: ChannelId = Snowflake(122);
pub const CH_GENERAL: ChannelId = Snowflake(130);
pub const CH_RANDOM: ChannelId = Snowflake(131);
pub const CH_HELP: ChannelId = Snowflake(132);
pub const CH_SHOWCASE: ChannelId = Snowflake(133);
/// Embed demo channels: video + link preview, and rich + photo grid.
pub const CH_EMBED_MEDIA: ChannelId = Snowflake(134);
pub const CH_EMBED_RICH: ChannelId = Snowflake(135);
pub const CH_EMBED_GRID: ChannelId = Snowflake(136);

pub const VC_LOUNGE: ChannelId = Snowflake(140);
pub const VC_PAIR: ChannelId = Snowflake(141);
pub const DM_ALICE: ChannelId = Snowflake(600);
pub const DM_BOB: ChannelId = Snowflake(601);
pub const DM_GROUP: ChannelId = Snowflake(602);
pub const DM_EVE: ChannelId = Snowflake(603);

pub const ROLE_MOD: RoleId = Snowflake(900);
pub const ROLE_MEMBER: RoleId = Snowflake(901);
pub const ROLE_CORE: RoleId = Snowflake(902);
pub const ROLE_BOT: RoleId = Snowflake(903);
pub const ROLE_GAMES_MODS: RoleId = Snowflake(4901);
pub const ROLE_GAMES_PLAYERS: RoleId = Snowflake(4902);
pub const MOD_COLOR: u32 = 0x2ecc71;

const NAMES: [(&str, &str); 7] = [
    ("", ""),
    ("ferris_fan", "Ferris Fan"),
    ("alice.dev", "Alice"),
    ("bobthebuilder", "Bob"),
    ("carol_codes", "Carol"),
    ("clippy_bot", "Clippy Bot"),
    ("eve", "Eve"),
];

/// Users already sitting in the mock voice channels.
pub fn voice_states() -> Vec<VoiceState> {
    let vs = |user_id: u64, channel: ChannelId, self_mute: bool, self_deaf: bool| VoiceState {
        guild_id: Some(GUILD_RUST),
        channel_id: Some(channel),
        user_id: Snowflake(user_id),
        session_id: format!("mock-voice-{user_id}"),
        self_mute,
        self_deaf,
        user: Some(user(user_id)),
        ..Default::default()
    };
    vec![
        vs(3, VC_LOUNGE, false, false),
        vs(4, VC_LOUNGE, true, false),
        vs(6, VC_PAIR, true, true),
    ]
}

/// Mock user `n` (1 = the logged-in user).
pub fn user(n: u64) -> User {
    let (u, g) = NAMES[(n as usize).min(NAMES.len() - 1)];
    let mut user = User {
        id: Snowflake(n),
        username: u.into(),
        global_name: Some(g.into()),
        avatar: None,
        bot: n == 5,
        ..Default::default()
    };
    cosmetics::decorate(&mut user);
    user
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(1_700_000_000_000)
}

/// Id of a message `minutes_ago` minutes before now.
fn ago(minutes: u64, inc: u64) -> MessageId {
    Snowflake::from_timestamp_ms(now_ms() - minutes * 60_000, inc)
}

fn ch(
    id: ChannelId,
    kind: ChannelKind,
    guild: Option<GuildId>,
    name: &str,
    pos: i32,
    parent: Option<ChannelId>,
) -> Channel {
    Channel {
        id,
        kind,
        guild_id: guild,
        name: (!name.is_empty()).then(|| name.to_owned()),
        topic: None,
        position: pos,
        parent_id: parent,
        last_message_id: None,
        recipients: vec![],
        nsfw: false,
        overwrites: vec![],
    }
}

fn guild(id: GuildId, name: &str, roles: Vec<Role>) -> Guild {
    Guild {
        id,
        name: name.into(),
        icon: None,
        roles,
        owner_id: Some(user(3).id),
        emojis: vec![],
        stickers: vec![],
        premium_tier: 0,
    }
}

fn custom_emoji(mut g: Guild) -> Guild {
    g.emojis = vec![
        CustomEmoji {
            id: Snowflake(1001),
            name: "ferris".into(),
            animated: false,
            available: true,
            roles: vec![],
        },
        CustomEmoji {
            id: Snowflake(1002),
            name: "party".into(),
            animated: false,
            available: true,
            roles: vec![],
        },
    ];
    g.stickers = mock_stickers();
    g
}

/// Emoji and stickers of the mock Iced server: from the Rust server they
/// are Nitro-locked for the mock user (no Nitro), so the pickers show the
/// lock and "Send as image".
fn iced_emoji(mut g: Guild) -> Guild {
    g.emojis = vec![
        CustomEmoji {
            id: Snowflake(1003),
            name: "iced_cube".into(),
            animated: false,
            available: true,
            roles: vec![],
        },
        CustomEmoji {
            id: Snowflake(1004),
            name: "spinner".into(),
            animated: true,
            available: true,
            roles: vec![],
        },
    ];
    g.stickers = vec![
        Sticker {
            id: Snowflake(1105),
            name: "Ice cube".into(),
            format: StickerFormat::Png,
            available: true,
        },
        Sticker {
            id: Snowflake(1106),
            name: "Melting".into(),
            format: StickerFormat::Lottie,
            available: true,
        },
    ];
    g
}

/// Stickers of the mock Rust server: three images (drawn by
/// [`mock_emoji`], preloaded in mock mode) and a Lottie one, which is shown
/// as a named placeholder.
pub fn mock_stickers() -> Vec<Sticker> {
    let s = |id: u64, name: &str, format: StickerFormat| Sticker {
        id: Snowflake(id),
        name: name.into(),
        format,
        available: true,
    };
    vec![
        s(1101, "Happy Ferris", StickerFormat::Png),
        s(1102, "Party Crab", StickerFormat::Png),
        s(1103, "Shipped it", StickerFormat::Apng),
        s(1104, "Wave", StickerFormat::Lottie),
    ]
}

/// Discord's default `@everyone` permissions.
pub const EVERYONE_PERMISSIONS: u64 = 1_071_698_660_929;

fn role(id: RoleId, name: &str, color: u32, position: i32) -> Role {
    Role {
        id,
        name: name.into(),
        color,
        position,
        permissions: 0,
        hoist: false,
        mentionable: false,
    }
}

/// A READY payload with five guilds, categories, DMs and read states.
pub fn ready() -> ReadyData {
    use ChannelKind::*;
    let g = Some(GUILD_RUST);
    let mut channels = vec![
        ch(CAT_INFO, Category, g, "Information", 0, None),
        ch(CAT_TEXT, Category, g, "Text Channels", 1, None),
        ch(CAT_VOICE, Category, g, "Voice Channels", 2, None),
        ch(CH_WELCOME, Text, g, "welcome", 0, Some(CAT_INFO)),
        ch(
            CH_ANNOUNCEMENTS,
            Announcement,
            g,
            "announcements",
            1,
            Some(CAT_INFO),
        ),
        ch(CH_RULES, Text, g, "rules", 2, Some(CAT_INFO)),
        ch(CH_GENERAL, Text, g, "general", 0, Some(CAT_TEXT)),
        ch(CH_RANDOM, Text, g, "random", 1, Some(CAT_TEXT)),
        ch(CH_HELP, Text, g, "help", 2, Some(CAT_TEXT)),
        ch(CH_SHOWCASE, Text, g, "showcase", 3, Some(CAT_TEXT)),
        ch(CH_EMBED_MEDIA, Text, g, "embeds-media", 4, Some(CAT_TEXT)),
        ch(CH_EMBED_RICH, Text, g, "embeds-rich", 5, Some(CAT_TEXT)),
        ch(CH_EMBED_GRID, Text, g, "embeds-grid", 6, Some(CAT_TEXT)),
        ch(Snowflake(140), Voice, g, "Lounge", 0, Some(CAT_VOICE)),
        ch(
            Snowflake(141),
            Voice,
            g,
            "Pair Programming",
            1,
            Some(CAT_VOICE),
        ),
    ];
    for (gid, base) in [
        (GUILD_ICED, 210u64),
        (GUILD_LINUX, 310),
        (GUILD_GAMES, 410),
        (GUILD_CAFE, 510),
    ] {
        channels.push(ch(
            Snowflake(base),
            Category,
            Some(gid),
            "Text Channels",
            0,
            None,
        ));
        for (i, n) in ["general", "help", "off-topic"].iter().enumerate() {
            channels.push(ch(
                Snowflake(base + 1 + i as u64),
                Text,
                Some(gid),
                n,
                i as i32,
                Some(Snowflake(base)),
            ));
        }
    }
    channels
        .iter_mut()
        .find(|c| c.id == CH_GENERAL)
        .into_iter()
        .for_each(|c| c.topic = Some("Talk about anything Rust. Be kind, be patient.".into()));
    channels
        .iter_mut()
        .find(|c| c.id == CH_SHOWCASE)
        .into_iter()
        .for_each(|c| c.topic = Some("Show off what you built".into()));

    let mut dm = |id: ChannelId, kind: ChannelKind, name: &str, recipients: Vec<u64>| {
        let mut c = ch(id, kind, None, name, 0, None);
        c.recipients = recipients.into_iter().map(Snowflake).collect();
        channels.push(c);
    };
    dm(DM_ALICE, Dm, "", vec![2]);
    dm(DM_BOB, Dm, "", vec![3]);
    dm(DM_GROUP, GroupDm, "Weekend plans", vec![4, 5, 6]);
    dm(DM_EVE, Dm, "", vec![6]);

    // Last message ids: everything has recent activity.
    let mut read_states = Vec::new();
    for (i, c) in channels.iter_mut().enumerate() {
        if c.kind.is_textual() {
            c.last_message_id = Some(ago(5 + i as u64 * 7, 0));
        }
    }
    // Read everything, then make a few channels unread / mentioned.
    for c in &channels {
        if let Some(last) = c.last_message_id {
            read_states.push((
                c.id,
                ReadState {
                    last_read: Some(last),
                    mention_count: 0,
                },
            ));
        }
    }
    let mut set = |id: ChannelId, unread: bool, mentions: u32| {
        if let Some((_, rs)) = read_states.iter_mut().find(|(c, _)| *c == id) {
            if unread {
                rs.last_read = Some(Snowflake(1));
            }
            rs.mention_count = mentions;
        }
    };
    set(CH_ANNOUNCEMENTS, true, 1);
    set(CH_RANDOM, true, 0);
    set(CH_HELP, true, 2);
    set(Snowflake(211), true, 0); // Iced: unread
    set(Snowflake(311), true, 3); // Linux: 3 mentions
    set(Snowflake(512), true, 0); // Cafe: unread
    set(DM_ALICE, true, 2);
    set(DM_GROUP, true, 1);

    ReadyData {
        voice_states: voice_states(),
        me: Some(user(1)),
        session_id: "mock-session".into(),
        resume_gateway_url: None,
        guilds: vec![
            custom_emoji(guild(
                GUILD_RUST,
                "Rust Programming",
                vec![
                    role(ROLE_MEMBER, "Member", 0, 1),
                    role(ROLE_MOD, "Moderator", MOD_COLOR, 3),
                    role(ROLE_CORE, "Core Team", 0xe67e22, 4),
                    role(ROLE_BOT, "Bot", 0x9b59b6, 2),
                ],
            )),
            iced_emoji(guild(GUILD_ICED, "Iced GUI", vec![])),
            guild(GUILD_LINUX, "Linux Desktop", vec![]),
            // Owned by the mock user: the server dropdown offers the admin
            // actions (Server Settings, Invite People).
            Guild {
                owner_id: Some(ME),
                ..guild(
                    GUILD_GAMES,
                    "Game Night",
                    vec![
                        Role {
                            permissions: EVERYONE_PERMISSIONS,
                            ..role(GUILD_GAMES, "@everyone", 0, 0)
                        },
                        Role {
                            permissions: 1 << 1 | 1 << 40 | 1 << 13,
                            ..role(ROLE_GAMES_MODS, "Moderators", 0xe91e63, 2)
                        },
                        role(ROLE_GAMES_PLAYERS, "Players", 0x3498db, 1),
                    ],
                )
            },
            guild(GUILD_CAFE, "Open Source Café", vec![]),
        ],
        channels,
        users: (2..=6).map(user).collect(),
        read_states,
        guild_order: vec![GUILD_RUST, GUILD_ICED, GUILD_LINUX, GUILD_GAMES, GUILD_CAFE],
        my_roles: vec![(GUILD_RUST, vec![ROLE_MEMBER])],
        muted_guilds: vec![],
        muted_channels: vec![],
        suppress_everyone: vec![],
        suppress_roles: vec![],
        activities: (2..=4)
            .map(|n| (Snowflake(n), cosmetics::activities(n)))
            .collect(),
        presences: vec![
            (Snowflake(2), Presence::Online),
            (Snowflake(3), Presence::Idle),
            (Snowflake(4), Presence::Dnd),
        ],
        my_status: Some(Presence::Online),
        premium_type: 0,
        guild_folders: vec![],
        channel_notify: vec![],
    }
}

/// Member list of the mock Rust server, as the server would send it.
pub fn member_list() -> MemberListUpdate {
    let member = |n: u64, roles: &[RoleId], presence: Presence| {
        MemberItem::Member(Box::new(MemberEntry {
            user: user(n),
            nick: None,
            roles: roles.to_vec(),
            presence,
            activities: cosmetics::activities(n),
        }))
    };
    let group = |id: &str, count: u32| MemberItem::Group {
        id: id.into(),
        count,
    };
    let groups = vec![
        (ROLE_CORE.to_string(), 1),
        (ROLE_MOD.to_string(), 1),
        (ROLE_BOT.to_string(), 1),
        ("online".to_string(), 2),
        ("offline".to_string(), 1),
    ];
    MemberListUpdate {
        guild_id: GUILD_RUST,
        list_id: "everyone".into(),
        member_count: 6,
        online_count: 5,
        groups,
        ops: vec![MemberOp::Sync {
            start: 0,
            items: vec![
                group(&ROLE_CORE.to_string(), 1),
                member(2, &[ROLE_CORE], Presence::Online),
                group(&ROLE_MOD.to_string(), 1),
                member(3, &[ROLE_MOD], Presence::Idle),
                group(&ROLE_BOT.to_string(), 1),
                member(5, &[ROLE_BOT], Presence::Online),
                group("online", 2),
                member(1, &[ROLE_MEMBER], Presence::Online),
                member(4, &[ROLE_MEMBER], Presence::Dnd),
                group("offline", 1),
                member(6, &[], Presence::Offline),
            ],
        }],
    }
}

/// A plain text message.
pub fn text_message(channel: ChannelId, id: MessageId, author: User, content: &str) -> Message {
    Message {
        id,
        channel_id: channel,
        guild_id: None,
        author,
        content: content.into(),
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
    }
}

/// A call service message (type 3) `minutes` ago lasting `lasted` minutes.
fn call_message(
    channel: ChannelId,
    minutes: u64,
    author: u64,
    participants: &[u64],
    lasted: Option<u64>,
) -> Message {
    let mut m = text_message(channel, ago(minutes, 0), user(author), "");
    m.kind = 3;
    m.call = Some(CallMessage {
        participants: participants.iter().map(|u| Snowflake(*u)).collect(),
        ended_ms: lasted.map(|l| m.id.timestamp_ms() + l * 60_000),
    });
    m
}

fn gm(
    channel: ChannelId,
    minutes: u64,
    inc: u64,
    author: u64,
    roles: &[RoleId],
    content: &str,
) -> Message {
    let mut m = text_message(channel, ago(minutes, inc), user(author), content);
    m.guild_id = if channel.0 < 600 {
        Some(GUILD_RUST)
    } else {
        None
    };
    m.author_roles = roles.to_vec();
    m
}

/// Message history for a channel, oldest first.
pub fn history(channel: ChannelId) -> Vec<Message> {
    match channel {
        CH_SHOWCASE => showcase(),
        CH_EMBED_MEDIA => embed_media_demo(),
        CH_EMBED_RICH => embed_rich_demo(),
        CH_EMBED_GRID => embed_grid_demo(),
        DM_ALICE => vec![
            gm(
                DM_ALICE,
                300,
                0,
                2,
                &[],
                "hey! did you see the new iced release?",
            ),
            gm(
                DM_ALICE,
                299,
                1,
                2,
                &[],
                "the **rich text** support is so good",
            ),
            gm(
                DM_ALICE,
                240,
                0,
                1,
                &[],
                "yes! I'm rewriting my client with it right now",
            ),
            gm(
                DM_ALICE,
                239,
                1,
                1,
                &[],
                "the Elm architecture makes the gateway events trivial to wire up",
            ),
            call_message(DM_ALICE, 120, 2, &[1, 2], Some(17)),
            gm(
                DM_ALICE,
                30,
                0,
                2,
                &[],
                "nice, can you share a screenshot later? 👀",
            ),
            gm(
                DM_ALICE,
                29,
                1,
                2,
                &[],
                "also ||I already bought the ferris plushie||",
            ),
            gm(
                DM_ALICE,
                20,
                0,
                1,
                &[],
                "notes for tomorrow:\n- ship the **composer**\n- fix Discover\n\nShift+Enter makes a new line",
            ),
            {
                let mut m = gm(
                    DM_ALICE,
                    19,
                    0,
                    2,
                    &[],
                    "🎉 great! 👍🏽 ❤️ 🇪🇸 1️⃣ 👨‍👩‍👧 🦀 café ☕",
                );
                m.reactions = [("🎉", 2, true), ("👍", 1, false), ("❤️", 3, false)]
                    .into_iter()
                    .map(|(e, count, me)| Reaction {
                        emoji: Emoji::unicode(e),
                        count,
                        me,
                    })
                    .collect();
                m
            },
        ],
        DM_GROUP => vec![
            gm(DM_GROUP, 90, 0, 4, &[], "Board games on Saturday?"),
            gm(DM_GROUP, 85, 0, 6, &[], "I'm in. I'll bring snacks 🍕"),
            gm(
                DM_GROUP,
                60,
                0,
                5,
                &[],
                "Reminder set for **Saturday 18:00**.",
            ),
        ],
        DM_BOB => long_history(DM_BOB),
        DM_EVE => vec![gm(DM_EVE, 4400, 0, 6, &[], "see you at the meetup")],
        _ => general(channel),
    }
}

/// Avatar hash given to the users of the long mock DM, so their avatars are
/// real raster images (see [`mock_emoji`]).
pub const MOCK_AVATAR: &str = "mockavatar";

/// A long, image-heavy conversation (300 messages: text of varying length,
/// code blocks, image attachments, raster avatars) to exercise scrolling
/// and rendering the way a real DM does.
pub fn long_history(channel: ChannelId) -> Vec<Message> {
    const LINES: [&str; 8] = [
        "thanks for the review yesterday!",
        "anytime 👍",
        "did you try the new build on Windows? scrolling feels much smoother now",
        "I think the cache eviction is off by one, look at this:
```rust
while self.bytes > self.budget && self.order.len() > 1 {
    self.evict();
}
```",
        "lol",
        "having a **huge** library makes me understand why people want a lighter client, it adds up fast when you keep ten of these open all day",
        "> quoted reply
and an answer below it with `inline code` and a [link](https://example.com)",
        "ok",
    ];
    let mut out = Vec::with_capacity(300);
    for i in 0..300u64 {
        let author = if i % 3 == 0 { 1 } else { 3 };
        let mut m = gm(
            channel,
            3000 - i * 9,
            0,
            author,
            &[],
            LINES[(i % 8) as usize],
        );
        m.author.avatar = Some(MOCK_AVATAR.into());
        if i % 8 == 5 {
            m.attachments = vec![Attachment {
                id: Snowflake(7000 + i),
                filename: "screenshot.png".into(),
                size: 482_133,
                url: "mock://image/1".into(),
                proxy_url: None,
                content_type: Some("image/png".into()),
                width: Some(400),
                height: Some(225),
            }];
        }
        out.push(m);
    }
    out
}

fn general(channel: ChannelId) -> Vec<Message> {
    let mut v = vec![
        gm(
            channel,
            180,
            0,
            3,
            &[ROLE_MOD],
            "Morning everyone! Reminder: please read <#122> before posting in <#132>.",
        ),
        gm(
            channel,
            62,
            0,
            4,
            &[ROLE_MEMBER],
            "Has anyone tried the new `let` chains in edition 2024?",
        ),
        gm(
            channel,
            61,
            1,
            4,
            &[ROLE_MEMBER],
            "they make nested `if let` *so* much nicer",
        ),
        gm(
            channel,
            58,
            0,
            2,
            &[ROLE_CORE],
            "Yes! Example:\n```rust\nif let Some(user) = cache.get(&id)\n    && let Some(name) = user.global_name.as_deref()\n{\n    println!(\"hello {name}\");\n}\n```",
        ),
        gm(
            channel,
            57,
            1,
            2,
            &[ROLE_CORE],
            "Docs: https://doc.rust-lang.org/edition-guide/",
        ),
        gm(
            channel,
            40,
            0,
            6,
            &[],
            "> they make nested `if let` so much nicer\nagreed, __finally__ ~~no more rightward drift~~ 🎉",
        ),
        gm(
            channel,
            12,
            0,
            5,
            &[ROLE_BOT],
            "**Tip of the day:** run `cargo clippy -- -D warnings` before every push.",
        ),
        gm(
            channel,
            3,
            0,
            3,
            &[ROLE_MOD],
            "<@1> can you take a look at the pinned question in <#132>? ||thanks!|| <:ferris:1001>",
        ),
    ];
    v[7].mentions = vec![ME];
    v[2].edited = true;
    let react = |emoji: Emoji, count: u32, me: bool| Reaction { emoji, count, me };
    v[3].reactions = vec![
        react(Emoji::unicode("👍"), 3, true),
        react(Emoji::unicode("🎉"), 2, false),
        react(
            Emoji {
                id: Some(Snowflake(1001)),
                name: "ferris".into(),
                animated: false,
            },
            1,
            false,
        ),
    ];
    // A reply from the logged-in user, so edit/delete and the reply header
    // are visible in screenshots.
    let mut reply = gm(
        channel,
        50,
        0,
        1,
        &[ROLE_MEMBER],
        "agreed, and `let` chains stabilised smoothly too",
    );
    reply.kind = 19;
    reply.reply_to = Some(v[1].id);
    reply.referenced = Some(Box::new(ReplyPreview::from_message(&v[1])));
    v.insert(5, reply);
    v.iter_mut().for_each(|m| m.channel_id = channel);
    v
}

fn showcase() -> Vec<Message> {
    let mut a = gm(
        CH_SHOWCASE,
        20,
        0,
        4,
        &[ROLE_MEMBER],
        "Finished my terminal file manager! Release notes: https://example.com/fm/releases/v1.0",
    );
    a.embeds = vec![Embed {
        kind: Some("rich".into()),
        title: Some("fm v1.0 — a tiny terminal file manager".into()),
        description: Some(
            "Fast, keyboard-driven and written in **100% safe Rust**.\nSupports previews, bulk rename and `git` status."
                .into(),
        ),
        url: Some("https://example.com/fm/releases/v1.0".into()),
        color: Some(0xe67e22),
        author_name: Some("example.com".into()),
        author_url: None,
        provider_name: Some("Example Releases".into()),
        footer: Some("Released today".into()),
        fields: vec![
            EmbedField {
                name: "Downloads".into(),
                value: "12,340".into(),
                inline: true,
            },
            EmbedField {
                name: "Stars".into(),
                value: "1.2k".into(),
                inline: true,
            },
            EmbedField {
                name: "License".into(),
                value: "MIT".into(),
                inline: true,
            },
        ],
        ..Default::default()
    }];
    let mut b = gm(
        CH_SHOWCASE,
        60,
        0,
        2,
        &[ROLE_CORE],
        "Screenshot of the GUI I'm working on:",
    );
    b.attachments = vec![Attachment {
        id: Snowflake(5001),
        filename: "screenshot.png".into(),
        size: 482_133,
        url: "mock://image/1".into(),
        proxy_url: None,
        content_type: Some("image/png".into()),
        width: Some(400),
        height: Some(225),
    }];
    let mut c = gm(
        CH_SHOWCASE,
        59,
        1,
        2,
        &[ROLE_CORE],
        "and the design doc if anyone wants to review",
    );
    c.attachments = vec![Attachment {
        id: Snowflake(5002),
        filename: "design-notes.pdf".into(),
        size: 1_532_211,
        url: "mock://file/2".into(),
        proxy_url: None,
        content_type: Some("application/pdf".into()),
        width: None,
        height: None,
    }];
    let mut d = gm(CH_SHOWCASE, 95, 0, 5, &[ROLE_BOT], "");
    d.embeds = vec![Embed {
        kind: Some("rich".into()),
        title: Some("Weekly build report".into()),
        description: Some("All **42** crates built successfully. ✅".into()),
        url: None,
        color: Some(0x57f287),
        author_name: Some("Clippy Bot".into()),
        author_url: None,
        provider_name: None,
        footer: Some("CI • ubuntu-latest".into()),
        fields: vec![
            EmbedField {
                name: "Duration".into(),
                value: "3m 12s".into(),
                inline: true,
            },
            EmbedField {
                name: "Warnings".into(),
                value: "0".into(),
                inline: true,
            },
        ],
        ..Default::default()
    }];
    let e = gm(
        CH_SHOWCASE,
        5,
        0,
        6,
        &[],
        "that looks amazing <:party:1002> <:ferris:1001>",
    );
    // A GIF link: shown as the bare GIF, without the URL or an embed card.
    let gif_url = "https://tenor.example/view/celebrate-123";
    let mut f = gm(CH_SHOWCASE, 3, 0, 2, &[], gif_url);
    f.embeds = vec![Embed {
        kind: Some("gifv".into()),
        url: Some(gif_url.into()),
        provider_name: Some("Tenor".into()),
        // Loops muted like a GIF (`app::media_anim`, `mock_gifv_frames`).
        video: Some(EmbedVideo {
            url: MOCK_GIFV.into(),
            proxy_url: None,
            width: Some(MOCK_GIFV_SIZE.0),
            height: Some(MOCK_GIFV_SIZE.1),
        }),
        thumbnail: Some(EmbedMedia {
            url: "mock://gif/1".into(),
            proxy_url: None,
            width: Some(220),
            height: Some(165),
        }),
        ..Default::default()
    }];
    // A sticker-only message, and one with a Lottie sticker (placeholder).
    let mut g = gm(CH_SHOWCASE, 4, 0, 3, &[], "");
    g.stickers = vec![mock_stickers()[1].clone()];
    let mut h = gm(CH_SHOWCASE, 2, 0, 4, &[], "");
    h.stickers = vec![mock_stickers()[3].clone()];
    // A video attachment: the poster is `mock://video/poster`; playing it
    // starts the scripted mock player instead of downloading.
    let mut v = gm(
        CH_SHOWCASE,
        1,
        0,
        2,
        &[ROLE_CORE],
        "demo of the new renderer",
    );
    v.attachments = vec![Attachment {
        id: Snowflake(5003),
        filename: "renderer-demo.mp4".into(),
        size: 7_412_902,
        url: "mock://video/1".into(),
        proxy_url: Some("mock://video/poster".into()),
        content_type: Some("video/mp4".into()),
        width: Some(1280),
        height: Some(720),
    }];
    // An animated GIF attachment (plays by itself, see `app::media_anim`).
    let mut i = gm(CH_SHOWCASE, 0, 1, 5, &[], "");
    i.attachments = vec![Attachment {
        id: Snowflake(5101),
        filename: "bounce.gif".into(),
        size: 96_512,
        url: MOCK_ANIM_GIF.into(),
        proxy_url: None,
        content_type: Some("image/gif".into()),
        width: Some(MOCK_ANIM_SIZE.0),
        height: Some(MOCK_ANIM_SIZE.1),
    }];
    vec![d, b, c, a, e, f, g, h, v, i]
}

/// The MP4 of the mock `gifv` embed (its frames come from
/// [`mock_gifv_frames`]; the still is `mock://gif/1`).
pub const MOCK_GIFV: &str = "mock://gifv/celebrate.mp4";
pub const MOCK_GIFV_SIZE: (u32, u32) = (220, 165);

/// Frames of the mock `gifv`: a rainbow pinwheel turning, 24 frames at
/// 25 fps (what the helper's frames mode hands over for an MP4).
pub fn mock_gifv_frames() -> Vec<(u32, u32, Vec<u8>, std::time::Duration)> {
    use std::f32::consts::TAU;
    let (w, h) = MOCK_GIFV_SIZE;
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    (0..24u32)
        .map(|f| {
            let turn = f as f32 / 24.0 * TAU / 3.0;
            let mut px = Vec::with_capacity((w * h * 4) as usize);
            for y in 0..h {
                for x in 0..w {
                    let (dx, dy) = (x as f32 - cx, y as f32 - cy);
                    let a = dy.atan2(dx) + turn;
                    let c = if dx.hypot(dy) < 70.0 {
                        let [r, g, b] = hue_rgb((a / TAU).rem_euclid(1.0));
                        let k = if (a * 3.0).sin() > 0.0 { 1.0 } else { 0.55 };
                        [(r * k) as u8, (g * k) as u8, (b * k) as u8, 255]
                    } else {
                        [32, 34, 52, 255]
                    };
                    px.extend_from_slice(&c);
                }
            }
            (w, h, px, std::time::Duration::from_millis(40))
        })
        .collect()
}

/// A saturated colour for `hue` in 0..1, as 0..255 floats.
fn hue_rgb(hue: f32) -> [f32; 3] {
    let h = hue * 6.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    [r * 255.0, g * 255.0, b * 255.0]
}

/// Key of the mock animated GIF attachment.
pub const MOCK_ANIM_GIF: &str = "mock://anim/bounce.gif";
pub const MOCK_ANIM_SIZE: (u32, u32) = (240, 160);

/// Frames of the mock GIF: a ball bouncing across a gradient, 12 frames.
pub fn mock_anim_frames() -> Vec<(u32, u32, Vec<u8>, std::time::Duration)> {
    let (w, h) = MOCK_ANIM_SIZE;
    (0..12u32)
        .map(|f| {
            let t = f as f32 / 12.0;
            let cx = 30.0 + t * (w as f32 - 60.0);
            let cy = h as f32 - 30.0 - (t * std::f32::consts::PI).sin() * (h as f32 - 60.0);
            let mut px = Vec::with_capacity((w * h * 4) as usize);
            for y in 0..h {
                for x in 0..w {
                    let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                    let c = if d < 22.0 {
                        [242, 63, 66, 255]
                    } else {
                        let g = (60.0 + 80.0 * y as f32 / h as f32) as u8;
                        [g / 2, g / 2, g, 255]
                    };
                    px.extend_from_slice(&c);
                }
            }
            (w, h, px, std::time::Duration::from_millis(80))
        })
        .collect()
}

/// Images behind the embed demo channels: (key, width, height, hue shift).
pub const MOCK_EMBED_IMAGES: [(&str, u32, u32, u8); 8] = [
    ("mock://embed/yt", 480, 270, 1),
    ("mock://embed/icon", 48, 48, 2),
    ("mock://embed/thumb", 128, 128, 0),
    ("mock://embed/tw1", 400, 267, 0),
    ("mock://embed/tw2", 400, 267, 1),
    ("mock://embed/tw3", 400, 267, 2),
    ("mock://embed/tw4", 400, 267, 3),
    ("mock://embed/link", 96, 96, 2),
];

/// Pixels for one of [`MOCK_EMBED_IMAGES`]: the mock landscape with its
/// colour channels rotated by `shift`.
pub fn mock_embed_image(w: u32, h: u32, shift: u8) -> (u32, u32, Vec<u8>) {
    let (w, h, mut px) = mock_image(w, h);
    for c in px.as_chunks_mut::<4>().0 {
        let (r, g, b) = (c[0], c[1], c[2]);
        let rot = match shift % 4 {
            1 => (g, b, r),
            2 => (b, r, g),
            3 => (b, g, r),
            _ => (r, g, b),
        };
        c[0] = rot.0;
        c[1] = rot.1;
        c[2] = rot.2;
    }
    (w, h, px)
}

fn proxied(key: &str, w: u32, h: u32) -> EmbedMedia {
    EmbedMedia {
        url: format!("https://original.example/{key}"),
        proxy_url: Some(key.into()),
        width: Some(w),
        height: Some(h),
    }
}

/// A YouTube video card and a link preview with a small thumbnail.
fn embed_media_demo() -> Vec<Message> {
    let yt_url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    let mut a = gm(CH_EMBED_MEDIA, 30, 0, 2, &[], yt_url);
    a.embeds = vec![Embed {
        kind: Some("video".into()),
        url: Some(yt_url.into()),
        title: Some("Writing a Discord client in Rust (iced + tokio)".into()),
        provider_name: Some("YouTube".into()),
        provider_url: Some("https://www.youtube.com".into()),
        author_name: Some("Ferris Talks".into()),
        author_url: Some("https://www.youtube.com/@ferristalks".into()),
        color: Some(0xff0000),
        thumbnail: Some(proxied("mock://embed/yt", 480, 270)),
        video: Some(EmbedVideo {
            url: "https://www.youtube.com/embed/dQw4w9WgXcQ".into(),
            proxy_url: None,
            width: Some(1280),
            height: Some(720),
        }),
        ..Default::default()
    }];
    let link = "https://blog.example.com/posts/zero-cost-abstractions";
    let mut b = gm(CH_EMBED_MEDIA, 10, 0, 3, &[], link);
    b.embeds = vec![Embed {
        kind: Some("link".into()),
        url: Some(link.into()),
        title: Some("Zero-cost abstractions, explained".into()),
        description: Some(
            "What the compiler really does with your iterators, closures and generics, with assembly.".into(),
        ),
        provider_name: Some("Example Blog".into()),
        thumbnail: Some(proxied("mock://embed/link", 96, 96)),
        ..Default::default()
    }];
    vec![a, b]
}

/// A bot embed with every decoration, and a Twitter-style photo set.
fn embed_rich_demo() -> Vec<Message> {
    let mut a = gm(CH_EMBED_RICH, 30, 0, 5, &[ROLE_BOT], "");
    a.embeds = vec![Embed {
        kind: Some("rich".into()),
        title: Some("Deploy finished: ferrocord v0.10.0".into()),
        description: Some(
            "All **6** jobs passed. Release notes are [here](https://example.com/notes).".into(),
        ),
        url: Some("https://example.com/deploys/42".into()),
        color: Some(0x5865f2),
        author_name: Some("Release Bot".into()),
        author_url: Some("https://example.com/bot".into()),
        author_icon: Some("mock://embed/icon".into()),
        thumbnail: Some(proxied("mock://embed/thumb", 128, 128)),
        fields: vec![
            EmbedField {
                name: "Duration".into(),
                value: "3m 12s".into(),
                inline: true,
            },
            EmbedField {
                name: "Tests".into(),
                value: "412 passed".into(),
                inline: true,
            },
            EmbedField {
                name: "Branch".into(),
                value: "`main`".into(),
                inline: true,
            },
            EmbedField {
                name: "Notes".into(),
                value: "Nothing flaky this time.".into(),
                inline: false,
            },
        ],
        footer: Some("CI • ubuntu-latest".into()),
        footer_icon: Some("mock://embed/icon".into()),
        timestamp_ms: Some(now_ms()),
        ..Default::default()
    }];
    vec![a]
}

/// A Twitter-style photo set: four embeds sharing one URL.
fn embed_grid_demo() -> Vec<Message> {
    let tw = "https://x.example.com/ferris/status/1";
    let mut b = gm(CH_EMBED_GRID, 10, 0, 2, &[], tw);
    let mut lead = Embed {
        kind: Some("rich".into()),
        url: Some(tw.into()),
        title: Some("Ferris (@ferris)".into()),
        description: Some("Four photos from the meetup!".into()),
        color: Some(0x1d9bf0),
        provider_name: Some("Example Social".into()),
        image: Some(proxied("mock://embed/tw1", 400, 267)),
        ..Default::default()
    };
    lead.footer = Some("Example Social".into());
    let mut embeds = vec![lead];
    for k in 2..=4 {
        embeds.push(Embed {
            kind: Some("rich".into()),
            url: Some(tw.into()),
            image: Some(proxied(&format!("mock://embed/tw{k}"), 400, 267)),
            ..Default::default()
        });
    }
    b.embeds = embeds;
    vec![b]
}

/// A small round "emoji" image (original artwork: a coloured face) used for
/// the mock custom emoji. Returns (size, pixels).
pub fn mock_emoji(rgb: [u8; 3]) -> (u32, Vec<u8>) {
    let n = 48u32;
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    let c = (n as f32 - 1.0) / 2.0;
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f32 - c, y as f32 - c);
            let d = (dx * dx + dy * dy).sqrt();
            let eye = ((dx.abs() - 8.0).powi(2) + (dy + 6.0).powi(2)).sqrt() < 3.5;
            let mouth = (d - 12.0).abs() < 2.0 && dy > 4.0;
            let p = if d > c {
                [0, 0, 0, 0]
            } else if eye || mouth {
                [40, 30, 30, 255]
            } else {
                [rgb[0], rgb[1], rgb[2], 255]
            };
            px.extend_from_slice(&p);
        }
    }
    (n, px)
}

/// Sizes of the mock GIF previews (`mock://gif/<n>`, n from 1).
pub const MOCK_GIF_SIZES: [(u32, u32); 8] = [
    (220, 124),
    (200, 200),
    (240, 135),
    (180, 240),
    (220, 165),
    (240, 120),
    (200, 150),
    (160, 160),
];

/// Page URL of mock GIF `n` (what gets sent; keys its picker still).
pub fn gif_page_url(n: usize) -> String {
    format!("https://tenor.example/view/mock-{n}")
}

/// Number of the mock GIF with this page URL.
pub fn gif_number(url: &str) -> Option<usize> {
    let n: usize = url
        .strip_prefix("https://tenor.example/view/mock-")?
        .parse()
        .ok()?;
    (1..=MOCK_GIF_SIZES.len()).contains(&n).then_some(n)
}

/// The animation of mock GIF `n` (a slow pan across its landscape, 32
/// frames at 50 ms), run through the picker's decode limits. Called off the
/// UI thread, like a real decode.
pub fn gif_anim(n: usize) -> Option<crate::app::anim::Anim> {
    let (w, h) = *MOCK_GIF_SIZES.get(n.checked_sub(1)?)?;
    let (bw, _, base) = mock_image(w * 2, h);
    let frames = (0..32u32)
        .map(|f| {
            let off = (f * w / 32) as usize;
            let mut px = Vec::with_capacity((w * h * 4) as usize);
            for y in 0..h as usize {
                let row = (y * bw as usize + off) * 4;
                px.extend_from_slice(&base[row..row + w as usize * 4]);
            }
            (w, h, px, std::time::Duration::from_millis(50))
        })
        .collect();
    crate::app::anim::from_frames_limited(frames, &crate::app::anim::Class::Picker.limits()).ok()
}

/// Saved GIFs of the mock account: mock GIFs 3 and 6, newest first.
pub fn gif_favorites() -> Vec<crate::net::frecency::FavoriteGif> {
    [(3usize, 2u32), (6, 1)]
        .into_iter()
        .map(|(n, order)| {
            let (w, h) = MOCK_GIF_SIZES[n - 1];
            crate::net::frecency::FavoriteGif {
                url: gif_page_url(n),
                src: format!("mock://gif/{n}.mp4"),
                format: crate::net::frecency::GifFormat::Video,
                width: w,
                height: h,
                order,
            }
        })
        .collect()
}

/// Mock GIF picker results: eight "trending" GIFs, four for any search.
pub fn gifs(query: &str) -> Vec<Gif> {
    let n = if query.trim().is_empty() { 8 } else { 4 };
    MOCK_GIF_SIZES
        .iter()
        .take(n)
        .enumerate()
        .map(|(i, (w, h))| Gif {
            url: gif_page_url(i + 1),
            preview: format!("mock://gif/{}", i + 1),
            gif_src: format!("mock://gif/{}", i + 1),
            src: format!("mock://gif/{}.mp4", i + 1),
            width: *w,
            height: *h,
        })
        .collect()
}

/// Procedurally generated RGBA image (a stylised landscape) used as the
/// mock attachment preview. Returns (width, height, pixels).
pub fn mock_image(w: u32, h: u32) -> (u32, u32, Vec<u8>) {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let fy = y as f32 / h as f32;
            let fx = x as f32 / w as f32;
            let hill = 0.62 + 0.08 * (fx * 9.0).sin() + 0.05 * (fx * 23.0).cos();
            let (r, g, b) = if fy > hill {
                (40.0 + 30.0 * fx, 110.0 + 50.0 * (1.0 - fy), 80.0)
            } else {
                let sun = ((fx - 0.72).powi(2) + (fy - 0.3).powi(2)).sqrt();
                if sun < 0.08 {
                    (255.0, 214.0, 120.0)
                } else {
                    (88.0 + 120.0 * fy, 101.0 + 60.0 * fy, 242.0 - 60.0 * fy)
                }
            };
            px.extend_from_slice(&[r as u8, g as u8, b as u8, 255]);
        }
    }
    (w, h, px)
}

/// (id, name, description, category, members, online, verified, partnered)
type DiscoverRow = (u64, &'static str, &'static str, u32, u32, u32, bool, bool);

// -- add a server / discover (mock mode) ----------------------------------------------

/// Preview for an invite code in mock mode: any code but `invalid` works.
pub fn invite_preview(code: &str) -> Option<crate::net::extras::InvitePreview> {
    if code.eq_ignore_ascii_case("invalid") {
        return None;
    }
    Some(crate::net::extras::InvitePreview {
        code: code.into(),
        guild_id: Snowflake(910_001),
        name: "Rustaceans".into(),
        icon: None,
        description: Some("Crabs, compilers and friendly help.".into()),
        members: Some(48_213),
        online: Some(9_870),
        inviter: Some("Ferris".into()),
        channel: Some("welcome".into()),
        verified: true,
        partnered: false,
    })
}

pub fn discover_categories() -> Vec<crate::net::extras::DiscoverCategory> {
    [
        (0, "Home"),
        (1, "Gaming"),
        (2, "Music"),
        (3, "Education"),
        (5, "Science & Tech"),
        (6, "Content Creator"),
        (4, "Entertainment"),
    ]
    .into_iter()
    .map(|(id, name)| crate::net::extras::DiscoverCategory {
        id,
        name: name.into(),
    })
    .collect()
}

pub fn discover_page(query: &str, category: Option<u32>) -> crate::net::extras::DiscoverPage {
    use crate::net::extras::{DiscoverGuild, DiscoverPage};
    let all: [DiscoverRow; 8] = [
        (
            910_101,
            "Rust Community",
            "Everything Rust: help, crates, jobs and the occasional crab pun.",
            5,
            48_213,
            9_870,
            true,
            true,
        ),
        (
            910_102,
            "Iced Users",
            "Building native GUIs with iced. Show off your widgets.",
            5,
            2_410,
            388,
            false,
            false,
        ),
        (
            910_103,
            "Linux Desktop",
            "KDE, GNOME, tiling window managers and dotfiles.",
            5,
            31_020,
            5_212,
            true,
            false,
        ),
        (
            910_104,
            "Study Together",
            "Quiet focus rooms and weekly accountability sessions.",
            3,
            12_300,
            1_940,
            false,
            false,
        ),
        (
            910_105,
            "Lo-fi Lounge",
            "Chill beats to code to. Share what you are listening to.",
            2,
            8_870,
            1_103,
            false,
            true,
        ),
        (
            910_106,
            "Board Game Night",
            "Weekly sessions of Catan, Wingspan and more.",
            1,
            5_432,
            702,
            false,
            false,
        ),
        (
            910_107,
            "Open Source Café",
            "Maintainers and contributors swapping stories.",
            5,
            3_980,
            611,
            false,
            false,
        ),
        (
            910_108,
            "Pixel Artists",
            "Share sprites, get feedback, join jams.",
            6,
            14_760,
            2_233,
            true,
            false,
        ),
    ];
    let q = query.trim().to_lowercase();
    let guilds: Vec<DiscoverGuild> = all
        .into_iter()
        .filter(|g| category.is_none_or(|c| c == 0 || g.3 == c))
        .filter(|g| {
            q.is_empty() || g.1.to_lowercase().contains(&q) || g.2.to_lowercase().contains(&q)
        })
        .map(
            |(id, name, desc, _, members, online, verified, partnered)| DiscoverGuild {
                id: Snowflake(id),
                name: name.into(),
                description: Some(desc.into()),
                icon: None,
                splash: None,
                members: Some(members),
                online: Some(online),
                verified,
                partnered,
            },
        )
        .collect();
    DiscoverPage {
        total: Some(guilds.len() as u32),
        guilds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_data_is_consistent() {
        let r = ready();
        for c in &r.channels {
            if let Some(g) = c.guild_id {
                assert!(
                    r.guilds.iter().any(|x| x.id == g),
                    "channel {} has unknown guild",
                    c.id
                );
            }
        }
        assert!(!history(CH_GENERAL).is_empty());
        assert!(history(CH_SHOWCASE).iter().any(|m| !m.embeds.is_empty()));
        assert!(history(CH_EMBED_MEDIA).iter().any(|m| !m.embeds.is_empty()));
        assert!(history(CH_EMBED_GRID).iter().any(|m| m.embeds.len() == 4));
        let (w, h, px) = mock_image(10, 5);
        assert_eq!(px.len(), (w * h * 4) as usize);
    }
}

pub use cosmetics::{profile, profile_effects};

/// Mock cosmetics, profiles and the images behind them (procedural: no
/// Discord asset is bundled).
pub mod cosmetics {
    use std::time::Duration;

    use super::*;
    use crate::app::anim::{self, Anim, Class};
    use crate::net::cosmetics::{
        self, Activity, ActivityKind, AvatarDecoration, Badge, Collectibles, EffectLayer,
        NameStyle, Nameplate, PrimaryGuild, Profile, ProfileEffect,
    };

    pub const CDN: &str = "https://cdn.discordapp.com";
    pub const AVATAR_ANIM: &str = "a_mockavatar";
    pub const DECO_ANIM: &str = "a_mockdeco";
    pub const DECO_STATIC: &str = "mockdeco";
    pub const PLATE: &str = "nameplates/mock/violet/";
    pub const TAG_BADGE: &str = "mocktagbadge";
    pub const BANNER: &str = "a_mockbanner";
    pub const EFFECT_ID: Snowflake = Snowflake(777);
    pub const EFFECT_SRC: &str =
        "https://cdn.discordapp.com/assets/profile_effects/configs/mock/sparkle.png";
    pub const SPOTIFY_ART: &str = "https://i.scdn.co/image/mockalbum";

    /// Gives mock users 1..=4 their cosmetics.
    pub fn decorate(u: &mut User) {
        match u.id.0 {
            1 => {
                u.avatar_decoration = Some(Box::new(AvatarDecoration {
                    asset: DECO_STATIC.into(),
                    sku_id: None,
                }));
            }
            2 => {
                u.avatar = Some(AVATAR_ANIM.into());
                u.avatar_decoration = Some(Box::new(AvatarDecoration {
                    asset: DECO_ANIM.into(),
                    sku_id: None,
                }));
                u.collectibles = Some(Box::new(Collectibles {
                    nameplate: Some(Nameplate {
                        asset: PLATE.into(),
                        palette: "violet".into(),
                        label: "Mock".into(),
                        sku_id: None,
                    }),
                }));
                u.primary_guild = Some(Box::new(PrimaryGuild {
                    identity_guild_id: Some(GUILD_RUST),
                    identity_enabled: Some(true),
                    tag: Some("RUST".into()),
                    badge: Some(TAG_BADGE.into()),
                }));
                u.name_style = Some(Box::new(NameStyle {
                    font_id: 11,
                    effect_id: 2,
                    colors: vec![0xff7eb3, 0x7ec8ff],
                }));
                u.banner = Some(BANNER.into());
            }
            3 => {
                u.avatar_decoration = Some(Box::new(AvatarDecoration {
                    asset: DECO_STATIC.into(),
                    sku_id: None,
                }));
                u.primary_guild = Some(Box::new(PrimaryGuild {
                    identity_guild_id: Some(GUILD_RUST),
                    identity_enabled: Some(true),
                    tag: Some("ICED".into()),
                    badge: None,
                }));
                u.name_style = Some(Box::new(NameStyle {
                    font_id: 14,
                    effect_id: 3,
                    colors: vec![0x7cffb2],
                }));
            }
            4 => {
                u.collectibles = Some(Box::new(Collectibles {
                    nameplate: Some(Nameplate {
                        asset: PLATE.into(),
                        palette: "teal".into(),
                        label: "Mock".into(),
                        sku_id: None,
                    }),
                }));
            }
            _ => {}
        }
    }

    pub fn activities(n: u64) -> Vec<Activity> {
        let base = |kind, name: &str| Activity {
            kind,
            name: name.into(),
            details: None,
            state: None,
            emoji: None,
            application_id: None,
            start_ms: None,
            end_ms: None,
            large_image: None,
            small_image: None,
        };
        match n {
            2 => vec![Activity {
                details: Some("Building a base".into()),
                state: Some("In game".into()),
                application_id: Some(Snowflake(7)),
                start_ms: Some(now_ms().saturating_sub(47 * 60_000 + 12_000)),
                large_image: Some("1234".into()),
                ..base(ActivityKind::Playing, "Factorio")
            }],
            3 => vec![Activity {
                details: Some("Borrowed Time".into()),
                state: Some("The Lifetimes".into()),
                start_ms: Some(now_ms().saturating_sub(95_000)),
                end_ms: Some(now_ms() + 120_000),
                large_image: Some("spotify:mockalbum".into()),
                ..base(ActivityKind::Listening, "Spotify")
            }],
            4 => vec![Activity {
                state: Some("heads down, shipping".into()),
                emoji: Some(("\u{1F680}".into(), None, false)),
                ..base(ActivityKind::Custom, "Custom Status")
            }],
            _ => vec![],
        }
    }

    /// The popout profile of mock user `n`.
    pub fn profile(user: UserId, guild: Option<GuildId>) -> Option<Profile> {
        let u = super::user(user.0);
        if user.0 == 0 || user.0 > 6 {
            return None;
        }
        let rich = user.0 == 2;
        Some(Profile {
            bio: if rich {
                "Rustacean, **iced** tinkerer and coffee fan :ferris: Ask me about lifetimes. https://example.com".into()
            } else {
                "Just here for the chat.".into()
            },
            pronouns: if rich {
                "she/her".into()
            } else {
                String::new()
            },
            banner: u.banner.clone(),
            banner_guild: None,
            accent_color: Some(0x5865f2),
            theme_colors: rich.then_some((0x3b1d5e, 0x1d4a6e)),
            effect: rich.then_some(EFFECT_ID),
            badges: if rich {
                vec![
                    Badge {
                        id: "premium".into(),
                        description: "Subscriber".into(),
                        icon: "mockicon1".into(),
                    },
                    Badge {
                        id: "early".into(),
                        description: "Early supporter".into(),
                        icon: "mockicon2".into(),
                    },
                ]
            } else {
                vec![]
            },
            premium_since: rich.then(|| "2019-05-01T00:00:00+00:00".into()),
            nick: None,
            roles: if guild.is_some() {
                vec![ROLE_MOD, ROLE_CORE]
            } else {
                vec![]
            },
            joined_at: guild.map(|_| "2021-03-14T09:00:00+00:00".into()),
            mutual_guilds: vec![(GUILD_RUST, Some("Alice".into())), (GUILD_ICED, None)],
            mutual_friends: vec![super::user(3), super::user(4)],
            mutual_friends_count: 2,
            user: u,
        })
    }

    pub fn profile_effects() -> Vec<ProfileEffect> {
        vec![ProfileEffect {
            id: EFFECT_ID,
            title: "Sparkle".into(),
            layers: vec![EffectLayer {
                src: EFFECT_SRC.into(),
                randomized: vec![],
                looped: true,
                width: 170,
                height: 330,
                duration_ms: 1200,
                start_ms: 0,
                loop_delay_ms: 0,
                x: 0,
                y: 0,
                z: 1,
            }],
            static_frame: None,
        }]
    }

    fn rgba(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&f(x, y));
            }
        }
        px
    }

    /// Ring frame around an avatar, `phase` rotates the highlight.
    fn ring(n: u32, phase: f32) -> Vec<u8> {
        let c = (n as f32 - 1.0) / 2.0;
        rgba(n, n, |x, y| {
            let (dx, dy) = (x as f32 - c, y as f32 - c);
            let d = (dx * dx + dy * dy).sqrt();
            let outer = c;
            let inner = c * 0.80;
            if d > outer || d < inner {
                return [0, 0, 0, 0];
            }
            let a = dy.atan2(dx) + phase;
            let t = (a.sin() * 0.5 + 0.5).powi(2);
            [
                (120.0 + 135.0 * t) as u8,
                (80.0 + 100.0 * t) as u8,
                255,
                255,
            ]
        })
    }

    fn dot_face(n: u32, rgb: [u8; 3]) -> Vec<u8> {
        let (side, px) = super::mock_emoji(rgb);
        if side == n {
            return px;
        }
        // Nearest-neighbour resize to n.
        rgba(n, n, |x, y| {
            let (sx, sy) = (x * side / n, y * side / n);
            let i = ((sy * side + sx) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        })
    }

    /// Static images keyed by the URLs the views request.
    pub fn images(cdn: &str) -> Vec<(String, u32, u32, Vec<u8>)> {
        let cdn = cdn.trim_end_matches('/');
        let mut out = Vec::new();
        let mut push = |url: String, w: u32, h: u32, px: Vec<u8>| out.push((url, w, h, px));
        for n in 1..=4u64 {
            let u = super::user(n);
            if let Some(d) = &u.avatar_decoration {
                for size in [anim_sizes::DEC_LIST, anim_sizes::DEC_POP] {
                    push(
                        d.url(cdn, size, false),
                        size.min(96),
                        size.min(96),
                        ring(size.min(96), 0.0),
                    );
                }
            }
            if n == 2 {
                for size in [64, 80, anim_sizes::AV_POP] {
                    if let Some(url) = cosmetics::avatar_url(cdn, &u, size) {
                        push(url, 48, 48, dot_face(48, [255, 126, 179]));
                    }
                }
                push(
                    cosmetics::banner_url(cdn, u.id, None, BANNER, false, anim_sizes::BANNER),
                    340,
                    120,
                    rgba(340, 120, |x, y| {
                        let t = x as f32 / 340.0;
                        [
                            (80.0 + 120.0 * t) as u8,
                            (60.0 + 40.0 * (y as f32 / 120.0)) as u8,
                            (200.0 - 90.0 * t) as u8,
                            255,
                        ]
                    }),
                );
                for icon in ["mockicon1", "mockicon2"] {
                    push(
                        cosmetics::badge_icon_url(cdn, icon),
                        48,
                        48,
                        dot_face(48, [250, 200, 70]),
                    );
                }
                if let Some(g) = &u.primary_guild
                    && let Some(url) = g.badge_url(cdn, 32)
                {
                    push(url, 48, 48, dot_face(48, [90, 200, 120]));
                }
                push(
                    format!("{cdn}/app-assets/7/1234.png"),
                    48,
                    48,
                    dot_face(48, [240, 120, 60]),
                );
            }
            if let Some(n) = u.collectibles.as_ref().and_then(|c| c.nameplate.as_ref()) {
                let rgb = n.palette_color();
                push(
                    n.static_url(cdn),
                    240,
                    44,
                    rgba(240, 44, |x, _| {
                        let f = x as f32 / 240.0;
                        let k = |c: u32| ((c as f32) * (0.35 + 0.65 * f)) as u8;
                        [
                            k((rgb >> 16) & 0xff),
                            k((rgb >> 8) & 0xff),
                            k(rgb & 0xff),
                            255,
                        ]
                    }),
                );
            }
        }
        push(SPOTIFY_ART.into(), 48, 48, dot_face(48, [30, 215, 96]));
        out
    }

    /// Sizes mirrored from `app::cosmetics` so this module stays free of
    /// app internals.
    mod anim_sizes {
        pub const DEC_LIST: u32 = crate::app::cosmetics::DEC_LIST;
        pub const DEC_POP: u32 = crate::app::cosmetics::DEC_POP;
        pub const AV_POP: u32 = crate::app::cosmetics::AV_POP;
        pub const BANNER: u32 = crate::app::cosmetics::BANNER_SIZE;
    }

    /// Animations kept for mock mode: avatar and decoration of mock user 2.
    pub fn anims(cdn: &str) -> Vec<(String, Class, Anim)> {
        let cdn = cdn.trim_end_matches('/');
        let u = super::user(2);
        let mut out = Vec::new();
        let colors = [
            [255u8, 126, 179],
            [126, 200, 255],
            [255, 220, 120],
            [160, 255, 190],
        ];
        // Like a real animated avatar: a 60-frame GIF at 30 ms, run
        // through the same limits as a downloaded one.
        let face = |n: u32, class: Class| {
            let frames = (0..60)
                .map(|i| {
                    (
                        n,
                        n,
                        dot_face(n, colors[(i / 15) % colors.len()]),
                        Duration::from_millis(30),
                    )
                })
                .collect();
            anim::from_frames_limited(frames, &class.limits()).ok()
        };
        for (size, class) in [(anim_sizes::AV_POP, Class::Avatar), (80, Class::AvatarList)] {
            if let Some(url) = cosmetics::avatar_anim_url(cdn, &u, size)
                && let Some(a) = face(48, class)
            {
                out.push((url, class, a));
            }
        }
        // Like a real decoration: a 3 s APNG at 60 fps (180 frames).
        if let Some(d) = &u.avatar_decoration {
            for (size, class) in [
                (anim_sizes::DEC_LIST, Class::DecorationList),
                (anim_sizes::DEC_POP, Class::Decoration),
            ] {
                let n = size.min(96);
                let frames = (0..180)
                    .map(|i| {
                        (
                            n,
                            n,
                            ring(n, i as f32 * std::f32::consts::TAU / 180.0),
                            Duration::from_nanos(1_000_000_000 / 60),
                        )
                    })
                    .collect();
                if let Ok(a) = anim::from_frames_limited(frames, &class.limits()) {
                    out.push((d.url(cdn, size, true), class, a));
                }
            }
        }
        out
    }

    /// Popout-only animations of mock user 2 (banner and effect): dropped
    /// when the popout closes, so they are re-created when it opens.
    pub fn popout_anims(cdn: &str) -> Vec<(String, Class, Anim)> {
        let cdn = cdn.trim_end_matches('/');
        let u = super::user(2);
        let banner = cosmetics::banner_url(cdn, u.id, None, BANNER, true, anim_sizes::BANNER);
        let b = anim::from_rgba_frames(
            (0..8)
                .map(|i| {
                    (
                        340,
                        120,
                        rgba(340, 120, |x, y| {
                            let t = ((x as f32 / 340.0) + i as f32 / 8.0) % 1.0;
                            [
                                (80.0 + 120.0 * t) as u8,
                                (60.0 + 40.0 * (y as f32 / 120.0)) as u8,
                                (200.0 - 90.0 * t) as u8,
                                255,
                            ]
                        }),
                        Duration::from_millis(120),
                    )
                })
                .collect(),
        );
        let effect = anim::from_rgba_frames(
            (0..10)
                .map(|i| {
                    (
                        170,
                        330,
                        rgba(170, 330, |x, y| {
                            // Falling sparkles.
                            let col = (x * 7 + 13) % 29 == 0;
                            let row = (y + i * 33) % 330;
                            if col && row % 41 < 5 {
                                [255, 240, 160, 230]
                            } else {
                                [0, 0, 0, 0]
                            }
                        }),
                        Duration::from_millis(120),
                    )
                })
                .collect(),
        );
        vec![
            (banner, Class::Banner, b),
            (EFFECT_SRC.into(), Class::Effect, effect),
        ]
    }
}
