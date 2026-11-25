//! Gateway payload parsing against recorded-shape fixtures.

use ferrocord::app::state::{ChatState, Ctx, Effect};
use ferrocord::net::Event;
use ferrocord::net::extras::NotifyLevel;
use ferrocord::net::model::*;
use ferrocord::net::types::parse_dispatch;
use serde_json::json;

fn fixture(name: &str) -> serde_json::Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn sf(s: &str) -> Snowflake {
    s.parse().unwrap()
}

fn ready() -> ReadyData {
    match parse_dispatch("READY", fixture("ready.json"))
        .unwrap()
        .unwrap()
    {
        Event::Ready(r) => *r,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn ready_user_account_layout() {
    let r = ready();
    assert_eq!(r.me.as_ref().unwrap().display_name(), "Ferris Fan");
    assert_eq!(r.session_id, "8d6b2f0f2c3e4a61a8f7c3b2a1d0e9f8");
    assert_eq!(
        r.resume_gateway_url.as_deref(),
        Some("wss://gateway-us-east1-b.discord.gg")
    );
    // Unavailable guild skipped; `properties` and legacy layouts both work.
    assert_eq!(r.guilds.len(), 2);
    assert_eq!(r.guilds[0].name, "Rust Programming");
    assert_eq!(r.guilds[0].icon.as_deref(), Some("iconhash"));
    assert_eq!(r.guilds[1].name, "Legacy Layout Guild");
    // Channels get their guild id even though the wire omits it.
    let general = r
        .channels
        .iter()
        .find(|c| c.id == sf("4000000000000000002"))
        .unwrap();
    assert_eq!(general.guild_id, Some(sf("2000000000000000001")));
    assert_eq!(general.kind, ChannelKind::Text);
    assert_eq!(general.parent_id, Some(sf("4000000000000000001")));
    let news = r
        .channels
        .iter()
        .find(|c| c.id == sf("4000000000000000010"))
        .unwrap();
    assert_eq!(news.kind, ChannelKind::Announcement);
}

#[test]
fn ready_private_channels_and_users() {
    let r = ready();
    let dm = r
        .channels
        .iter()
        .find(|c| c.id == sf("6000000000000000001"))
        .unwrap();
    assert_eq!(dm.kind, ChannelKind::Dm);
    assert_eq!(dm.recipients, vec![sf("1000000000000000002")]);
    let group = r
        .channels
        .iter()
        .find(|c| c.id == sf("6000000000000000002"))
        .unwrap();
    assert_eq!(group.kind, ChannelKind::GroupDm);
    assert_eq!(group.last_message_id, None);
    // Users from `users` and from embedded recipients are both collected.
    assert!(r.users.iter().any(|u| u.username == "alice.dev"));
    assert!(r.users.iter().any(|u| u.username == "carol_codes"));
}

#[test]
fn ready_read_states_settings_and_roles() {
    let r = ready();
    // Non-channel read states (type 1) are ignored; `0` ids mean "never".
    assert_eq!(r.read_states.len(), 2);
    let general = r
        .read_states
        .iter()
        .find(|(c, _)| *c == sf("4000000000000000002"))
        .unwrap()
        .1;
    assert_eq!(general.last_read, Some(sf("5000000000000000005")));
    assert_eq!(general.mention_count, 2);
    let dm = r
        .read_states
        .iter()
        .find(|(c, _)| *c == sf("6000000000000000001"))
        .unwrap()
        .1;
    assert_eq!(dm.last_read, None);

    assert_eq!(r.muted_guilds, vec![sf("2000000000000000002")]);
    assert_eq!(r.suppress_everyone, vec![sf("2000000000000000002")]);
    assert!(r.suppress_roles.is_empty());
    assert_eq!(r.muted_channels, vec![sf("4000000000000000003")]);
    // Overrides with a level are kept even when not muted; defaults are not.
    let mentions_only = r
        .channel_notify
        .iter()
        .find(|(c, _)| *c == sf("4000000000000000002"))
        .unwrap()
        .1;
    assert_eq!(mentions_only.level, NotifyLevel::Mentions);
    assert!(!mentions_only.muted);
    assert_eq!(r.channel_notify.len(), 2);
    // Folders keep id, name and colour; a null id is a single server.
    assert_eq!(r.guild_folders.len(), 2);
    assert_eq!(r.guild_folders[0].id, None);
    assert_eq!(r.guild_folders[1].id, Some(12345));
    assert_eq!(r.guild_folders[1].name.as_deref(), Some("Dev"));
    assert_eq!(r.guild_folders[1].color, Some(5_793_266));
    assert_eq!(r.guild_folders[1].guild_ids.len(), 2);
    assert_eq!(
        r.guild_order,
        vec![
            sf("2000000000000000002"),
            sf("2000000000000000001"),
            sf("2000000000000000003")
        ]
    );
    assert_eq!(
        r.my_roles[0],
        (sf("2000000000000000001"), vec![sf("3000000000000000001")])
    );
}

#[test]
fn message_create_full() {
    let Event::MessageCreate(m) = parse_dispatch("MESSAGE_CREATE", fixture("message_create.json"))
        .unwrap()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(m.author.display_name(), "Alice");
    assert_eq!(m.kind, 19);
    assert_eq!(m.reply_to, Some(sf("5000000000000000009")));
    assert_eq!(m.mentions, vec![sf("1000000000000000001")]);
    assert_eq!(m.author_roles, vec![sf("3000000000000000001")]);
    assert_eq!(m.nonce.as_deref(), Some("1234567890123"));
    assert!(!m.edited);
    let a = &m.attachments[0];
    assert!(a.is_image());
    assert_eq!((a.width, a.height), (Some(800), Some(600)));
    let e = &m.embeds[0];
    assert_eq!(e.color, Some(0xff0000));
    assert_eq!(e.author_name.as_deref(), Some("Author"));
    assert_eq!(e.footer.as_deref(), Some("Footer"));
    assert_eq!(e.fields.len(), 1);
    // Proxy URLs are what gets downloaded.
    assert!(
        e.thumbnail
            .as_ref()
            .unwrap()
            .download_url()
            .contains("images-ext-1.discordapp.net")
    );
    // Debug output never contains the body.
    assert!(!format!("{m:?}").contains("look at"));
}

#[test]
fn message_update_is_partial() {
    let Event::MessageUpdate(p) = parse_dispatch("MESSAGE_UPDATE", fixture("message_update.json"))
        .unwrap()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(p.content.as_deref(), Some("edited text"));
    assert!(p.edited);
    assert!(p.embeds.is_none());

    let Event::MessageUpdate(p) =
        parse_dispatch("MESSAGE_UPDATE", fixture("message_update_embeds_only.json"))
            .unwrap()
            .unwrap()
    else {
        panic!()
    };
    assert!(p.content.is_none());
    assert_eq!(p.embeds, Some(vec![]));
    assert!(!p.edited);
}

#[test]
fn typing_ack_delete_supplemental_guild_channel() {
    let Event::TypingStart { user, user_id, .. } =
        parse_dispatch("TYPING_START", fixture("typing_start.json"))
            .unwrap()
            .unwrap()
    else {
        panic!()
    };
    assert_eq!(user_id, sf("1000000000000000002"));
    assert_eq!(user.unwrap().display_name(), "Alice");

    assert!(matches!(
        parse_dispatch("MESSAGE_ACK", fixture("message_ack.json"))
            .unwrap()
            .unwrap(),
        Event::MessageAck {
            mention_count: 0,
            ..
        }
    ));
    assert!(matches!(
        parse_dispatch("MESSAGE_DELETE", fixture("message_delete.json"))
            .unwrap()
            .unwrap(),
        Event::MessageDelete { .. }
    ));
    let Event::MemberRoles(roles) =
        parse_dispatch("READY_SUPPLEMENTAL", fixture("ready_supplemental.json"))
            .unwrap()
            .unwrap()
    else {
        panic!()
    };
    assert_eq!(roles[0].1, sf("1000000000000000002"));

    let Event::GuildUpsert {
        guild,
        channels,
        partial,
        ..
    } = parse_dispatch("GUILD_CREATE", fixture("guild_create.json"))
        .unwrap()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(guild.name, "New Guild");
    assert_eq!(channels[0].guild_id, Some(guild.id));
    assert!(!partial);

    let Event::ChannelUpsert(c, users) =
        parse_dispatch("CHANNEL_CREATE", fixture("channel_create_dm.json"))
            .unwrap()
            .unwrap()
    else {
        panic!()
    };
    assert!(c.kind.is_private());
    assert_eq!(users[0].username, "dave");
}

#[test]
fn malformed_known_event_is_an_error_not_a_panic() {
    assert!(parse_dispatch("MESSAGE_CREATE", serde_json::json!({"id": "x"})).is_err());
    assert!(parse_dispatch("READY", serde_json::json!([])).is_err());
}

/// End to end through the reducer: READY then a mention.
#[test]
fn ready_then_mention_updates_unread_state() {
    let mut s = ChatState::default();
    let ctx = Ctx {
        now_ms: 0,
        open_channel: None,
        window_focused: true,
        notification_preview: true,
    };
    s.apply_event(&Event::Ready(Box::new(ready())), &ctx);
    let general = sf("4000000000000000002");
    assert!(s.is_unread(general));
    assert_eq!(s.mention_count(general), 2);
    // Muted voice channel and muted guild do not light up the rail.
    assert!(s.guild_badge(sf("2000000000000000001")).unread);
    assert!(!s.guild_badge(sf("2000000000000000002")).unread);

    let ev = parse_dispatch("MESSAGE_CREATE", fixture("message_create.json"))
        .unwrap()
        .unwrap();
    let fx = s.apply_event(&ev, &ctx);
    assert_eq!(s.mention_count(general), 3);
    assert!(matches!(&fx[0], Effect::Notify(n) if n.title.contains("#general")));
    // Author colour from the member roles in the event.
    let Event::MessageCreate(m) = ev else {
        panic!()
    };
    assert_eq!(s.author_color(m.guild_id, &m), Some(3066993));
}

// -- replies, reactions, presence, member list, emoji ---------------------------

fn parse(t: &str, f: &str) -> Event {
    parse_dispatch(t, fixture(f)).unwrap().unwrap()
}

fn message_of(t: &str, f: &str) -> Message {
    match parse(t, f) {
        Event::MessageCreate(m) => *m,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn reply_with_referenced_message() {
    let m = message_of("MESSAGE_CREATE", "message_create_reply.json");
    assert_eq!(m.kind, 19);
    assert_eq!(m.reply_to, Some(sf("5000000000000000019")));
    let r = m.referenced.expect("reply preview");
    assert!(!r.deleted);
    assert_eq!(r.message_id, Some(sf("5000000000000000019")));
    assert_eq!(r.author.as_ref().unwrap().display_name(), "Alice");
    assert_eq!(r.content, "Has anyone tried the new let chains?");
    assert!(r.has_attachment);
}

#[test]
fn reply_to_deleted_message() {
    let m = message_of("MESSAGE_CREATE", "message_create_reply_deleted.json");
    let r = m.referenced.expect("reply preview");
    assert!(r.deleted);
    assert_eq!(r.message_id, Some(sf("5000000000000000019")));
}

#[test]
fn non_reply_messages_have_no_header() {
    let m = message_of("MESSAGE_CREATE", "message_with_reactions.json");
    assert_eq!(m.kind, 0);
    assert!(m.referenced.is_none());
}

#[test]
fn reactions_are_parsed() {
    let m = message_of("MESSAGE_CREATE", "message_with_reactions.json");
    assert_eq!(m.reactions.len(), 3);
    assert_eq!(m.reactions[0].emoji, Emoji::unicode("👍"));
    assert_eq!((m.reactions[0].count, m.reactions[0].me), (3, true));
    assert_eq!(m.reactions[1].emoji.id, Some(sf("1001")));
    assert_eq!(m.reactions[1].emoji.api_segment(), "ferris:1001");
    assert!(!m.reactions[2].me);
}

fn state_with_reaction_message() -> (ChatState, MessageId, ChannelId) {
    let mut s = ChatState::default();
    s.load_ready(&ready());
    let m = message_of("MESSAGE_CREATE", "message_with_reactions.json");
    let (id, ch) = (m.id, m.channel_id);
    s.messages.entry(ch).or_default().upsert(m);
    (s, id, ch)
}

#[test]
fn reaction_events_update_counts() {
    let (mut s, id, ch) = state_with_reaction_message();
    let ctx = Ctx::default();
    let count = |s: &ChatState, e: &Emoji| {
        s.message(ch, id)
            .unwrap()
            .reactions
            .iter()
            .find(|r| r.emoji == *e)
            .map(|r| (r.count, r.me))
    };
    let thumbs = Emoji::unicode("👍");
    // Another user adds a thumbs up.
    s.apply_event(&parse("MESSAGE_REACTION_ADD", "reaction_add.json"), &ctx);
    assert_eq!(count(&s, &thumbs), Some((4, true)));
    // The custom emoji is removed for everybody.
    s.apply_event(
        &parse(
            "MESSAGE_REACTION_REMOVE_EMOJI",
            "reaction_remove_emoji.json",
        ),
        &ctx,
    );
    let ferris = s.message(ch, id).unwrap().reactions.len();
    assert_eq!(ferris, 2);
    // Remove-all clears the lot.
    s.apply_event(
        &Event::Reaction(ReactionEvent::RemoveAll {
            channel_id: ch,
            message_id: id,
        }),
        &ctx,
    );
    assert!(s.message(ch, id).unwrap().reactions.is_empty());
}

#[test]
fn own_reaction_echo_is_idempotent() {
    let (mut s, id, ch) = state_with_reaction_message();
    let me = s.me_id().unwrap();
    let party = Emoji::unicode("🎈");
    // Optimistic toggle, then the gateway echo of our own add.
    assert_eq!(
        s.toggle_reaction(ch, id, &party),
        Some(ferrocord::app::state::ReactionChange::Added)
    );
    let echo = Event::Reaction(ReactionEvent::Add {
        channel_id: ch,
        message_id: id,
        user_id: me,
        emoji: party.clone(),
    });
    s.apply_event(&echo, &Ctx::default());
    let r = s
        .message(ch, id)
        .unwrap()
        .reactions
        .iter()
        .find(|r| r.emoji == party)
        .unwrap();
    assert_eq!((r.count, r.me), (1, true));
    // Toggle off and echo the removal.
    assert_eq!(
        s.toggle_reaction(ch, id, &party),
        Some(ferrocord::app::state::ReactionChange::Removed)
    );
    s.apply_event(
        &Event::Reaction(ReactionEvent::Remove {
            channel_id: ch,
            message_id: id,
            user_id: me,
            emoji: party.clone(),
        }),
        &Ctx::default(),
    );
    assert!(
        s.message(ch, id)
            .unwrap()
            .reactions
            .iter()
            .all(|r| r.emoji != party)
    );
}

#[test]
fn failed_reaction_is_reverted() {
    let (mut s, id, ch) = state_with_reaction_message();
    let thumbs = Emoji::unicode("👍");
    // We had reacted (me = true): toggling removes, a failure restores.
    let change = s.toggle_reaction(ch, id, &thumbs).unwrap();
    assert_eq!(s.message(ch, id).unwrap().reactions[0].count, 2);
    s.revert_reaction(ch, id, &thumbs, change);
    let r = &s.message(ch, id).unwrap().reactions[0];
    assert_eq!((r.count, r.me), (3, true));
}

#[test]
fn presence_update_and_ready_presences() {
    let mut s = ChatState::default();
    s.load_ready(&ready());
    let alice = sf("1000000000000000002");
    assert_eq!(s.presence_of(alice), Presence::Offline);
    s.apply_event(
        &parse("PRESENCE_UPDATE", "presence_update.json"),
        &Ctx::default(),
    );
    assert_eq!(s.presence_of(alice), Presence::Dnd);
    // READY presences replace what we knew.
    let mut r = ready();
    r.presences = vec![(alice, Presence::Idle)];
    r.my_status = Some(Presence::Dnd);
    s.load_ready(&r);
    assert_eq!(s.presence_of(alice), Presence::Idle);
    assert_eq!(s.presence_of(s.me_id().unwrap()), Presence::Dnd);
}

#[test]
fn ready_presences_and_settings_status_are_parsed() {
    let mut d = fixture("ready.json");
    d["presences"] = serde_json::json!([
        { "user": { "id": "1000000000000000002" }, "status": "idle" },
        { "user_id": "1000000000000000003", "status": "online" },
        { "status": "online" }
    ]);
    d["user_settings"] = serde_json::json!({ "status": "dnd" });
    d["user"]["premium_type"] = serde_json::json!(2);
    let Event::Ready(r) = parse_dispatch("READY", d).unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(r.presences.len(), 2);
    assert_eq!(r.presences[0].1, Presence::Idle);
    assert_eq!(r.my_status, Some(Presence::Dnd));
    assert_eq!(r.premium_type, 2);
}

#[test]
fn member_list_update_is_converted_and_applied() {
    let Event::MemberList(u) = parse("GUILD_MEMBER_LIST_UPDATE", "member_list_update.json") else {
        panic!()
    };
    assert_eq!(u.member_count, 4);
    // The unknown op is dropped; the unknown item keeps its slot.
    assert_eq!(u.ops.len(), 5);
    let MemberOp::Sync { start, items } = &u.ops[0] else {
        panic!()
    };
    assert_eq!((*start, items.len()), (0, 7));
    let MemberItem::Member(alice) = &items[1] else {
        panic!()
    };
    assert_eq!(alice.display_name(), "Al");
    assert_eq!(alice.presence, Presence::Online);

    // Reduce all ops in order: after INVALIDATE nothing is loaded.
    let mut l = ferrocord::app::members::MemberListState::new(u.guild_id);
    let first_only = ferrocord::net::model::MemberListUpdate {
        ops: vec![u.ops[0].clone()],
        ..(*u).clone()
    };
    l.apply(&first_only);
    assert_eq!(l.loaded().count(), 7);
    l.apply(&ferrocord::net::model::MemberListUpdate {
        ops: u.ops[1..].to_vec(),
        ..(*u).clone()
    });
    assert_eq!(l.loaded().count(), 0);
}

#[test]
fn member_list_feeds_presence_and_users() {
    let mut s = ChatState::default();
    s.load_ready(&ready());
    let ev = parse("GUILD_MEMBER_LIST_UPDATE", "member_list_update.json");
    s.member_list = Some(ferrocord::app::members::MemberListState::new(sf(
        "2000000000000000001",
    )));
    s.apply_event(&ev, &Ctx::default());
    assert_eq!(s.presence_of(sf("1000000000000000003")), Presence::Idle);
    assert!(s.users.contains_key(&sf("1000000000000000004")));
}

#[test]
fn guild_emojis_are_loaded_and_updated() {
    let mut d = fixture("guild_create.json");
    d["emojis"] = serde_json::json!([{ "id": "1001", "name": "ferris" }]);
    d["premium_tier"] = serde_json::json!(2);
    let Event::GuildUpsert { guild, .. } = parse_dispatch("GUILD_CREATE", d).unwrap().unwrap()
    else {
        panic!()
    };
    assert_eq!(guild.emojis.len(), 1);
    assert!(guild.emojis[0].available, "available defaults to true");
    assert_eq!(guild.premium_tier, 2);

    let mut s = ChatState::default();
    s.guilds.push(*guild.clone());
    s.apply_event(
        &parse("GUILD_EMOJIS_UPDATE", "guild_emojis_update.json"),
        &Ctx::default(),
    );
    // The fixture guild id differs from GUILD_CREATE's: nothing changes...
    let gid = sf("2000000000000000001");
    s.guilds[0].id = gid;
    s.apply_event(
        &parse("GUILD_EMOJIS_UPDATE", "guild_emojis_update.json"),
        &Ctx::default(),
    );
    // ...and with the right id the list is replaced.
    assert_eq!(s.guild(gid).unwrap().emojis.len(), 2);
    assert!(s.guild(gid).unwrap().emojis[1].animated);
}

#[test]
fn malformed_new_events_do_not_panic() {
    for t in [
        "MESSAGE_REACTION_ADD",
        "MESSAGE_REACTION_REMOVE",
        "MESSAGE_REACTION_REMOVE_ALL",
        "GUILD_MEMBER_LIST_UPDATE",
        "GUILD_EMOJIS_UPDATE",
    ] {
        // Missing required ids: an error (logged by the gateway), no panic.
        let _ = parse_dispatch(t, serde_json::json!({}));
        let _ = parse_dispatch(t, serde_json::json!({"ops": "nope", "emoji": 7}));
    }
    // PRESENCE_UPDATE without a user is ignored.
    assert!(
        parse_dispatch("PRESENCE_UPDATE", serde_json::json!({"status": "online"}))
            .unwrap()
            .is_none()
    );
}

#[test]
fn user_settings_update_carries_folders() {
    let ev = ferrocord::net::types::parse_dispatch(
        "USER_SETTINGS_UPDATE",
        json!({"guild_folders": [{"id": 7, "name": "Work", "color": null, "guild_ids": ["1", "2"]}]}),
    )
    .unwrap()
    .unwrap();
    let Event::GuildFolders(f) = ev else {
        panic!("wrong event")
    };
    assert_eq!(f[0].id, Some(7));
    assert_eq!(f[0].color, None);
    // Status-only updates are not folder updates.
    assert!(
        ferrocord::net::types::parse_dispatch("USER_SETTINGS_UPDATE", json!({"status": "dnd"}))
            .unwrap()
            .is_none()
    );
}

#[test]
fn user_guild_settings_update_carries_channel_overrides() {
    let ev = ferrocord::net::types::parse_dispatch(
        "USER_GUILD_SETTINGS_UPDATE",
        json!({"guild_id": "9", "channel_overrides": [
            {"channel_id": "5", "muted": true, "message_notifications": 2,
             "mute_config": {"selected_time_window": 900, "end_time": "2099-01-01T00:00:00Z"}}
        ]}),
    )
    .unwrap()
    .unwrap();
    let Event::NotifySettings { guild, channels } = ev else {
        panic!("wrong event")
    };
    assert_eq!(guild, Some(sf("9")));
    assert_eq!(channels[0].1.level, NotifyLevel::Nothing);
    assert!(channels[0].1.is_muted(0));
    assert!(channels[0].1.mute_end_ms.is_some());
}

#[test]
fn stickers_parse_on_messages_guilds_and_updates() {
    use serde_json::json;
    let Ok(Some(Event::MessageCreate(m))) = parse_dispatch(
        "MESSAGE_CREATE",
        json!({"id": "10", "channel_id": "20", "content": "",
               "author": {"id": "2", "username": "alice"},
               "sticker_items": [{"id": "30", "name": "Wave", "format_type": 3},
                                 {"id": "31", "name": "Cat", "format_type": 4}]}),
    ) else {
        panic!("message with stickers did not parse");
    };
    assert_eq!(m.stickers.len(), 2);
    assert_eq!(m.stickers[0].format, StickerFormat::Lottie);
    assert_eq!(m.stickers[1].format, StickerFormat::Gif);

    let Ok(Some(Event::GuildStickers { guild_id, stickers })) = parse_dispatch(
        "GUILD_STICKERS_UPDATE",
        json!({"guild_id": "5", "stickers": [
            {"id": "40", "name": "Party", "format_type": 1, "available": false},
            {"id": "41", "name": "Ship it", "format_type": 2}]}),
    ) else {
        panic!("sticker update did not parse");
    };
    assert_eq!(guild_id, Snowflake(5));
    assert!(!stickers[0].available && stickers[1].available);
    assert_eq!(stickers[1].format, StickerFormat::Apng);

    let mut s = ChatState::default();
    s.guilds.push(Guild {
        id: Snowflake(5),
        name: "g".into(),
        icon: None,
        roles: vec![],
        owner_id: None,
        emojis: vec![],
        stickers: vec![],
        premium_tier: 0,
    });
    s.apply_event(
        &Event::GuildStickers { guild_id, stickers },
        &Ctx::default(),
    );
    assert_eq!(s.guild(Snowflake(5)).unwrap().stickers.len(), 2);
}

// -- voice and calls ----------------------------------------------------------

#[test]
fn voice_state_update_parses() {
    let Event::VoiceState(v) =
        parse_dispatch("VOICE_STATE_UPDATE", fixture("voice_state_update.json"))
            .unwrap()
            .unwrap()
    else {
        panic!()
    };
    assert_eq!(v.channel_id, Some(sf("1000000000000000140")));
    assert_eq!(v.guild_id, Some(sf("1000000000000000001")));
    assert!(v.mute && v.self_mute && !v.self_deaf);
    assert_eq!(v.user.as_ref().unwrap().display_name(), "Bob");
    // Leaving: channel_id null.
    let mut d = fixture("voice_state_update.json");
    d["channel_id"] = serde_json::Value::Null;
    let Event::VoiceState(v) = parse_dispatch("VOICE_STATE_UPDATE", d).unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(v.channel_id, None);
}

#[test]
fn voice_server_update_for_a_call() {
    let Event::VoiceServer(s) =
        parse_dispatch("VOICE_SERVER_UPDATE", fixture("voice_server_update.json"))
            .unwrap()
            .unwrap()
    else {
        panic!()
    };
    assert_eq!(s.guild_id, None);
    assert_eq!(s.channel_id, Some(sf("1000000000000000600")));
    assert_eq!(
        s.endpoint.as_deref(),
        Some("c-fra08-1234abcd.discord.media:443")
    );
    // The token never shows up in Debug output.
    assert!(!format!("{s:?}").contains("fixture-voice-token"));
    // A null endpoint (server being reallocated) is kept as None.
    let mut d = fixture("voice_server_update.json");
    d["endpoint"] = serde_json::Value::Null;
    let Event::VoiceServer(s) = parse_dispatch("VOICE_SERVER_UPDATE", d).unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(s.endpoint, None);
}

#[test]
fn call_events_parse() {
    let Event::Call(c) = parse_dispatch("CALL_CREATE", fixture("call_create.json"))
        .unwrap()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(c.channel_id, sf("1000000000000000600"));
    assert_eq!(c.ringing, vec![sf("1000000000000000001")]);
    assert_eq!(c.voice_states.len(), 1);
    assert_eq!(c.voice_states[0].user_id, sf("1000000000000000002"));
    let d = serde_json::json!({"channel_id": "1000000000000000600"});
    let Event::CallDelete(ch) = parse_dispatch("CALL_DELETE", d).unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(ch, sf("1000000000000000600"));
}

#[test]
fn call_service_message_parses() {
    let Event::MessageCreate(m) =
        parse_dispatch("MESSAGE_CREATE", fixture("message_create_call.json"))
            .unwrap()
            .unwrap()
    else {
        panic!()
    };
    assert_eq!(m.kind, 3);
    let call = m.call.as_ref().unwrap();
    assert_eq!(call.participants.len(), 2);
    let ended = call.ended_ms.unwrap();
    // 2026-11-20T10:05:30Z
    assert_eq!(ended % 1000, 0);
    assert_eq!(ended, 1_795_169_130_000);
}

#[test]
fn hostile_voice_payloads_do_not_panic() {
    for t in [
        "VOICE_STATE_UPDATE",
        "VOICE_SERVER_UPDATE",
        "CALL_CREATE",
        "CALL_UPDATE",
        "CALL_DELETE",
    ] {
        for d in [
            serde_json::json!(null),
            serde_json::json!({}),
            serde_json::json!({"user_id": "x"}),
            serde_json::json!({"channel_id": 5, "ringing": "no", "voice_states": [1]}),
            serde_json::json!({"endpoint": "x".repeat(10_000), "token": 5}),
        ] {
            let _ = parse_dispatch(t, d);
        }
    }
    // A huge endpoint is dropped rather than used.
    let d = serde_json::json!({"token": "t", "guild_id": "1", "endpoint": "a".repeat(1000)});
    let Event::VoiceServer(s) = parse_dispatch("VOICE_SERVER_UPDATE", d).unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(s.endpoint, None);
}

#[test]
fn ready_and_guild_create_carry_voice_states() {
    let mut g = fixture("guild_create.json");
    g["voice_states"] = serde_json::json!([fixture("voice_state_update.json")]);
    let Event::GuildUpsert { voice_states, .. } =
        parse_dispatch("GUILD_CREATE", g).unwrap().unwrap()
    else {
        panic!()
    };
    assert_eq!(voice_states.len(), 1);
    assert!(voice_states[0].guild_id.is_some());
}

#[test]
fn op4_payload_shape() {
    let p = ferrocord::net::types::voice_state_payload(None, Some(sf("600")), true, false);
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(v["op"], 4);
    assert_eq!(v["d"]["guild_id"], serde_json::Value::Null);
    assert_eq!(v["d"]["channel_id"], "600");
    assert_eq!(v["d"]["self_mute"], true);
    assert_eq!(v["d"]["self_deaf"], false);
    let leave = ferrocord::net::types::voice_state_payload(Some(sf("1")), None, false, false);
    assert_eq!(
        serde_json::to_value(&leave).unwrap()["d"]["channel_id"],
        serde_json::Value::Null
    );
}

// -- profile cosmetics ------------------------------------------------------------

#[test]
fn presence_update_carries_activities() {
    let mut d = fixture("presence_update.json");
    d["activities"] = json!([
        {"type": 4, "name": "Custom Status", "state": "away", "emoji": {"name": "zzz"}},
        {"type": 0, "name": "Factorio", "application_id": "7", "timestamps": {"start": 1700000000000u64}}
    ]);
    let Event::PresenceUpdate {
        user_id,
        status,
        activities,
    } = parse_dispatch("PRESENCE_UPDATE", d).unwrap().unwrap()
    else {
        panic!()
    };
    assert_eq!(
        (user_id, status),
        (sf("1000000000000000002"), Presence::Dnd)
    );
    assert_eq!(activities.len(), 2);
    let mut s = ChatState::default();
    s.set_activities(user_id, activities);
    assert_eq!(s.activities_of(user_id).len(), 2);
    // An update without activities clears them.
    s.apply_event(
        &parse("PRESENCE_UPDATE", "presence_update.json"),
        &Ctx::default(),
    );
    assert!(s.activities_of(user_id).is_empty());
}

#[test]
fn ready_presences_carry_activities() {
    let mut d = fixture("ready.json");
    d["presences"] = json!([
        {"user": {"id": "1000000000000000002"}, "status": "online",
         "activities": [{"type": 2, "name": "Spotify", "details": "Song"}]},
        {"user": {"id": "1000000000000000003"}, "status": "online", "activities": []}
    ]);
    let Event::Ready(r) = parse_dispatch("READY", d).unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(r.presences.len(), 2);
    assert_eq!(r.activities.len(), 1);
    let mut s = ChatState::default();
    s.load_ready(&r);
    assert_eq!(
        s.activities_of(sf("1000000000000000002"))[0].name,
        "Spotify"
    );
}

#[test]
fn message_authors_and_member_lists_carry_cosmetics() {
    let mut d = fixture("message_create.json");
    d["author"]["avatar_decoration_data"] =
        json!({"asset": "a_deco", "sku_id": "5", "expires_at": null});
    d["author"]["primary_guild"] =
        json!({"identity_guild_id": "9", "identity_enabled": true, "tag": "TAG", "badge": "b"});
    d["author"]["collectibles"] = json!({"nameplate": {"asset": "nameplates/x/y/", "palette": "sky", "label": "l", "sku_id": "1"}});
    d["author"]["display_name_styles"] =
        json!({"font_id": 11, "effect_id": 2, "colors": [255, 65280]});
    let Event::MessageCreate(m) = parse_dispatch("MESSAGE_CREATE", d).unwrap().unwrap() else {
        panic!()
    };
    let a = &m.author;
    assert_eq!(a.avatar_decoration.as_ref().unwrap().asset, "a_deco");
    assert_eq!(a.primary_guild.as_ref().unwrap().visible_tag(), Some("TAG"));
    assert!(a.collectibles.as_ref().unwrap().nameplate.is_some());
    assert_eq!(a.name_style.as_ref().unwrap().colors, vec![255, 65280]);

    // Member list: the member's server decoration wins, activities flow in,
    // and the user in the cache is refreshed.
    let mut d = fixture("member_list_update.json");
    // Every member object of alice (the fixture re-sends her later).
    fn decorate(v: &mut serde_json::Value, touched: &mut u32) {
        match v {
            serde_json::Value::Object(o) => {
                if o.get("user")
                    .is_some_and(|u| u["id"] == "1000000000000000002")
                    && o.contains_key("roles")
                {
                    o.insert(
                        "avatar_decoration_data".into(),
                        json!({"asset": "guilddeco"}),
                    );
                    o.insert(
                        "presence".into(),
                        json!({"status": "online", "activities": [{"type": 0, "name": "Game"}]}),
                    );
                    *touched += 1;
                }
                o.values_mut().for_each(|c| decorate(c, touched));
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(|c| decorate(c, touched)),
            _ => {}
        }
    }
    let mut touched = 0;
    decorate(&mut d, &mut touched);
    assert!(touched > 0);
    let ev = parse_dispatch("GUILD_MEMBER_LIST_UPDATE", d)
        .unwrap()
        .unwrap();
    let mut s = ChatState::default();
    s.load_ready(&ready());
    s.member_list = Some(ferrocord::app::members::MemberListState::new(sf(
        "2000000000000000001",
    )));
    s.apply_event(&ev, &Ctx::default());
    let alice = sf("1000000000000000002");
    assert_eq!(
        s.users[&alice].avatar_decoration.as_ref().unwrap().asset,
        "guilddeco"
    );
    assert_eq!(s.activities_of(alice)[0].name, "Game");
}

#[test]
fn guild_subscription_asks_for_activities() {
    let p = ferrocord::net::types::member_subscribe_payload(sf("1"), sf("2"), &[(0, 99)]);
    let s = serde_json::to_value(&p).unwrap();
    assert_eq!(s["d"]["activities"], json!(true));
}

#[test]
fn channel_overwrites_and_role_events() {
    let Event::ChannelUpsert(c, _) = parse_dispatch(
        "CHANNEL_UPDATE",
        json!({
            "id": "50", "type": 0, "guild_id": "10", "name": "staff",
            "permission_overwrites": [
                {"id": "10", "type": 0, "allow": "0", "deny": "1024"},
                {"id": "20", "type": "role", "allow": "1024", "deny": "0"},
                {"id": "1", "type": 1, "allow": "2048", "deny": "0"}
            ]
        }),
    )
    .unwrap()
    .unwrap() else {
        panic!()
    };
    assert_eq!(c.overwrites.len(), 3);
    assert_eq!(c.overwrites[0].deny, 1024);
    assert!(!c.overwrites[1].member);
    assert_eq!(c.overwrites[1].allow, 1024);
    assert!(c.overwrites[2].member);

    let Event::RoleUpsert { guild_id, role } = parse_dispatch(
        "GUILD_ROLE_UPDATE",
        json!({"guild_id": "10", "role": {"id": "20", "name": "Mods", "color": 5,
               "position": 2, "permissions": "8192"}}),
    )
    .unwrap()
    .unwrap() else {
        panic!()
    };
    assert_eq!(guild_id, sf("10"));
    assert_eq!((role.name.as_str(), role.permissions), ("Mods", 8192));

    assert!(matches!(
        parse_dispatch(
            "GUILD_ROLE_DELETE",
            json!({"guild_id": "10", "role_id": "20"})
        )
        .unwrap()
        .unwrap(),
        Event::RoleDelete { role_id, .. } if role_id == sf("20")
    ));

    let Event::MemberRoles(list) = parse_dispatch(
        "GUILD_MEMBER_UPDATE",
        json!({"guild_id": "10", "roles": ["20"], "user": {"id": "1", "username": "me"}}),
    )
    .unwrap()
    .unwrap() else {
        panic!()
    };
    assert_eq!(list, vec![(sf("10"), sf("1"), vec![sf("20")])]);
}
