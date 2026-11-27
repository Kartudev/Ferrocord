//! App-level flows of the member sidebar and the "who reacted" tooltip /
//! dialog, driven through `App::update` in mock mode.

use super::reactions::{ReactionKey, TOOLTIP_LIMIT, pill_kind};
use super::{App, Done, Message, Ui};
use crate::config::Config;
use crate::mock;
use crate::net::Event;
use crate::net::model::*;

fn app() -> App {
    App::boot(Config::for_tests()).0
}

fn member(id: u64) -> MemberItem {
    MemberItem::Member(Box::new(MemberEntry {
        user: User {
            id: Snowflake(id),
            username: format!("m{id}"),
            ..Default::default()
        },
        nick: None,
        roles: vec![],
        presence: Presence::Online,
        activities: vec![],
    }))
}

fn update(list: &str, ops: Vec<MemberOp>, online: u32) -> Event {
    Event::MemberList(Box::new(MemberListUpdate {
        guild_id: mock::GUILD_RUST,
        list_id: list.into(),
        member_count: online,
        online_count: online,
        groups: vec![("online".into(), online)],
        ops,
    }))
}

fn sync(start: usize, n: usize, first_id: u64) -> MemberOp {
    MemberOp::Sync {
        start,
        items: (0..n as u64).map(|i| member(first_id + i)).collect(),
    }
}

#[test]
fn scrolling_a_large_list_subscribes_the_visible_chunks() {
    let mut a = app();
    let _ = a.update(Message::Net(update(
        "everyone",
        vec![sync(0, 100, 10_000)],
        5000,
    )));
    let l = a.chat.member_list.as_ref().expect("list");
    assert_eq!(l.len(), 5001);
    assert_eq!(
        a.member_subscription().map(|s| s.ranges),
        Some(vec![(0, 99)])
    );
    // Scroll to row ~2500: the old code never asked past row 499.
    let rows = crate::ui::members::ROWS;
    let headers = l.header_rows();
    let y = rows.offset(&headers, 2500);
    let _ = a.members_scrolled(y, 800.0);
    let sub = a.member_subscription().expect("subscribed");
    assert_eq!(sub.ranges[0], (0, 99));
    assert!(sub.ranges.contains(&(2500, 2599)), "{:?}", sub.ranges);
    assert!(sub.ranges.len() <= super::members::MAX_RANGES);
    // Rows there are placeholders until the SYNC arrives; drawing them is
    // cheap (only the window is built).
    let w = a.member_window();
    assert!(w.start <= 2500 && w.end > 2500 && w.len() < 60, "{w:?}");
    let _ = crate::ui::view(&a);
    let _ = a.update(Message::Net(update(
        "everyone",
        vec![sync(2500, 100, 20_000)],
        5000,
    )));
    let l = a.chat.member_list.as_ref().expect("list");
    assert!(matches!(l.get(2500), Some(MemberItem::Member(m)) if m.user.id.0 == 20_000));
    // Memory stays bounded: only the subscribed ranges are kept.
    assert!(l.loaded_count() <= 300, "{}", l.loaded_count());
    let _ = crate::ui::view(&a);
}

#[test]
fn switching_channels_resubscribes_and_takes_the_new_list() {
    let mut a = app();
    let _ = a.update(Message::Ui(Ui::SelectChannel(mock::CH_HELP)));
    assert_eq!(
        a.member_subscription().map(|s| s.channel),
        Some(mock::CH_HELP)
    );
    // The channel has its own list (permission overwrites).
    let _ = a.update(Message::Net(update("4242", vec![sync(0, 3, 30_000)], 2)));
    let l = a.chat.member_list.as_ref().expect("list");
    assert_eq!(l.list_id.as_deref(), Some("4242"));
    let ids: Vec<u64> = l
        .loaded()
        .filter_map(|i| match i {
            MemberItem::Member(m) => Some(m.user.id.0),
            MemberItem::Group { .. } => None,
        })
        .collect();
    // Two members + the header slot (len 3); nothing of the old list.
    assert_eq!(l.len(), 3);
    assert!(ids.iter().all(|id| *id >= 30_000), "{ids:?}");
    // A late update of the previous channel's list is ignored.
    let _ = a.update(Message::Net(update(
        "everyone",
        vec![MemberOp::Insert {
            index: 1,
            item: member(99),
        }],
        3,
    )));
    let l = a.chat.member_list.as_ref().expect("list");
    assert_eq!(l.len(), 3);
}

fn reacted(a: &App) -> (ChannelId, MessageId, Reaction) {
    let w = &a.chat.messages[&mock::CH_GENERAL];
    let m = w
        .messages
        .iter()
        .find(|m| m.reactions.len() >= 2)
        .expect("a mock message with reactions");
    (mock::CH_GENERAL, m.id, m.reactions[0].clone())
}

#[test]
fn hovering_a_reaction_fetches_its_reactors_once_after_a_delay() {
    let mut a = app();
    let (c, m, r) = reacted(&a);
    let (burst, count) = pill_kind(&r);
    let key = ReactionKey::new(c, m, &r.emoji, burst);
    let _ = a.update(Message::Ui(Ui::ReactionHover(key.clone())));
    // Nothing is fetched on enter, only once the pointer rested.
    assert!(a.reactions.cache.get(&key).is_none());
    let generation = a.reactions.hover_generation;
    // A stale timer (the pointer moved on and came back) does nothing.
    let _ = a.update(Message::Done(Done::ReactionHoverDue(generation - 1)));
    assert!(a.reactions.cache.get(&key).is_none());
    let _ = a.update(Message::Done(Done::ReactionHoverDue(generation)));
    let e = a.reactions.cache.get(&key).expect("fetched");
    assert_eq!(e.users.len(), count.min(TOOLTIP_LIMIT) as usize);
    let _ = crate::ui::view(&a);
    // Leaving before the delay: no fetch for that pill.
    let (_, _, r2) = {
        let w = &a.chat.messages[&mock::CH_GENERAL];
        let m = w.messages.iter().find(|x| x.id == m).expect("message");
        (c, m.id, m.reactions[1].clone())
    };
    let key2 = ReactionKey::new(c, m, &r2.emoji, false);
    let _ = a.update(Message::Ui(Ui::ReactionHover(key2.clone())));
    let g2 = a.reactions.hover_generation;
    let _ = a.update(Message::Ui(Ui::ReactionUnhover(key2.clone())));
    let _ = a.update(Message::Done(Done::ReactionHoverDue(g2)));
    assert!(a.reactions.cache.get(&key2).is_none());
    // Someone we know reacts: the cached list follows without a refetch.
    let _ = a.update(Message::Net(Event::Reaction(ReactionEvent::Remove {
        channel_id: c,
        message_id: m,
        user_id: Snowflake(1),
        emoji: r.emoji.clone(),
        burst: false,
    })));
    let e = a.reactions.cache.get(&key).expect("still cached");
    assert!(e.users.iter().all(|u| u.id != Snowflake(1)));
    // REMOVE_ALL drops the lists of the message.
    let _ = a.update(Message::Net(Event::Reaction(ReactionEvent::RemoveAll {
        channel_id: c,
        message_id: m,
    })));
    assert!(a.reactions.cache.get(&key).is_none());
}

#[test]
fn reactions_dialog_has_a_tab_per_emoji_and_pages() {
    let mut a = app();
    let (_, m, r) = reacted(&a);
    let _ = a.update(Message::Ui(Ui::OpenReactors(m, None)));
    let d = a.reactions.dialog.clone().expect("dialog open");
    assert_eq!(d.emoji, r.emoji);
    let key = ReactionKey::new(d.channel, m, &d.emoji, d.burst);
    let e = a.reactions.cache.get(&key).expect("tab fetched");
    assert_eq!(e.users.len(), r.count as usize);
    assert!(e.complete);
    let _ = crate::ui::view(&a);
    // Another tab.
    let other = {
        let w = &a.chat.messages[&mock::CH_GENERAL];
        w.messages
            .iter()
            .find(|x| x.id == m)
            .expect("message")
            .reactions[1]
            .clone()
    };
    let _ = a.update(Message::Ui(Ui::ReactorsTab(other.emoji.clone(), false)));
    let k2 = ReactionKey::new(d.channel, m, &other.emoji, false);
    assert!(a.reactions.cache.get(&k2).is_some());
    let _ = crate::ui::view(&a);
    // From the message menu too; Escape closes it.
    let _ = a.update(Message::Ui(Ui::CloseReactors));
    assert!(a.reactions.dialog.is_none());
    let _ = a.update(Message::Ui(Ui::ContextMenu(m)));
    let _ = a.update(Message::Ui(Ui::OpenReactors(m, Some(other.emoji.clone()))));
    assert_eq!(
        a.reactions.dialog.as_ref().map(|d| d.emoji.clone()),
        Some(other.emoji)
    );
    assert!(a.menu.is_none());
    let _ = a.update(Message::Ui(Ui::Escape));
    assert!(a.reactions.dialog.is_none());
}
