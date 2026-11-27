//! Who reacted: the tooltip of a hovered reaction pill and the
//! "Reactions" dialog (one tab per emoji), like the official client.
//!
//! Reactors are fetched lazily with `GET /channels/{c}/messages/{m}/reactions/{emoji}`
//! (`type=1` for burst / super reactions): only after the pointer rested
//! on a pill for [`HOVER_DELAY_MS`], or when the dialog opens a tab. Never
//! for every message on screen. Results are cached in a small LRU
//! ([`MAX_ENTRIES`]) and patched or marked stale by MESSAGE_REACTION_* events.

use std::collections::HashMap;

use iced::Task;

use super::actions::delayed;
use super::{App, Done, Message, Ui};
use crate::net::model::*;

/// Pointer rest time before a hovered pill fetches its reactors (the
/// tooltip appears after the same delay).
pub const HOVER_DELAY_MS: u64 = 300;
/// Users fetched for a tooltip (it names at most [`TOOLTIP_NAMES`]).
pub const TOOLTIP_LIMIT: u32 = 3;
/// Names shown in a tooltip before "and N others".
pub const TOOLTIP_NAMES: usize = 3;
/// Users per page in the dialog (Discord's maximum).
pub const PAGE_LIMIT: u32 = 100;
/// Cached (message, emoji, kind) lists at most.
pub const MAX_ENTRIES: usize = 64;
/// Users kept per list at most (the dialog stops paging there).
pub const MAX_USERS: usize = 1000;

/// One reaction of one message. `burst` = super reaction (`type=1`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReactionKey {
    pub channel: ChannelId,
    pub message: MessageId,
    /// Emoji identity: the custom emoji id, or the Unicode sequence.
    pub emoji: String,
    pub burst: bool,
}

impl ReactionKey {
    pub fn new(channel: ChannelId, message: MessageId, emoji: &Emoji, burst: bool) -> Self {
        Self {
            channel,
            message,
            emoji: emoji_identity(emoji),
            burst,
        }
    }
}

/// Same identity rule as `Emoji::eq`: custom emoji by id, Unicode by text.
pub fn emoji_identity(e: &Emoji) -> String {
    match e.id {
        Some(id) => id.to_string(),
        None => e.name.clone(),
    }
}

/// Fetched reactors of one reaction.
#[derive(Debug, Clone, Default)]
pub struct Reactors {
    /// In Discord's order (ascending user id; pages continue with `after`).
    pub users: Vec<User>,
    /// Every reactor is in `users` (the last page was short).
    pub complete: bool,
    pub loading: bool,
    pub failed: bool,
    /// A reaction event changed the list after it was fetched: the next
    /// hover or dialog view refetches it (the old names stay meanwhile).
    pub stale: bool,
    used: u64,
}

/// Bounded cache of reactor lists.
#[derive(Debug, Default)]
pub struct ReactorCache {
    entries: HashMap<ReactionKey, Reactors>,
    clock: u64,
}

/// A fetch the app should start.
#[derive(Debug, Clone, PartialEq)]
pub struct Fetch {
    pub key: ReactionKey,
    pub limit: u32,
    pub after: Option<UserId>,
    /// The reaction count when the fetch started.
    pub count: u32,
}

impl ReactorCache {
    pub fn get(&self, key: &ReactionKey) -> Option<&Reactors> {
        self.entries.get(key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether `key` needs a fetch to show `wanted` users (capped by the
    /// reaction's `count`); marks it loading if so. A list that is loading
    /// is never fetched twice.
    pub fn want(&mut self, key: &ReactionKey, wanted: u32, count: u32) -> Option<Fetch> {
        self.clock += 1;
        let clock = self.clock;
        let wanted = wanted.min(count).min(MAX_USERS as u32) as usize;
        if let Some(e) = self.entries.get_mut(key) {
            e.used = clock;
            if e.loading {
                return None;
            }
            let enough = e.complete || e.users.len() >= wanted;
            if enough && !e.stale && !e.failed {
                return None;
            }
            e.loading = true;
            e.failed = false;
            // A stale or failed list starts over; a short one continues.
            let after = if e.stale || e.failed || e.users.is_empty() {
                None
            } else {
                e.users.last().map(|u| u.id)
            };
            let have = if after.is_some() { e.users.len() } else { 0 };
            let limit = (wanted.saturating_sub(have)).clamp(1, PAGE_LIMIT as usize) as u32;
            return Some(Fetch {
                key: key.clone(),
                limit: if after.is_some() { PAGE_LIMIT } else { limit },
                after,
                count,
            });
        }
        self.evict();
        self.entries.insert(
            key.clone(),
            Reactors {
                loading: true,
                used: clock,
                ..Default::default()
            },
        );
        Some(Fetch {
            key: key.clone(),
            limit: (wanted as u32).clamp(1, PAGE_LIMIT),
            after: None,
            count,
        })
    }

    /// Applies a finished fetch.
    pub fn finish(&mut self, f: &Fetch, result: Result<Vec<User>, ()>) {
        let Some(e) = self.entries.get_mut(&f.key) else {
            return;
        };
        e.loading = false;
        match result {
            Err(()) => e.failed = true,
            Ok(users) => {
                let short = (users.len() as u32) < f.limit;
                if f.after.is_none() {
                    e.users.clear();
                    e.stale = false;
                }
                for u in users {
                    if e.users.len() >= MAX_USERS {
                        break;
                    }
                    if !e.users.iter().any(|x| x.id == u.id) {
                        e.users.push(u);
                    }
                }
                e.complete = short || e.users.len() >= f.count as usize;
            }
        }
    }

    /// MESSAGE_REACTION_ADD. A complete list takes the user in id order
    /// when it is known; otherwise the list is refetched on next use.
    pub fn on_add(&mut self, message: MessageId, emoji: &Emoji, burst: bool, user: Option<&User>) {
        let id = emoji_identity(emoji);
        for (k, e) in self.entries.iter_mut() {
            if k.message != message || k.emoji != id || k.burst != burst {
                continue;
            }
            match user {
                Some(u) if e.users.iter().any(|x| x.id == u.id) => {}
                Some(u) if e.complete => {
                    let at = e.users.partition_point(|x| x.id < u.id);
                    e.users.insert(at, u.clone());
                }
                _ => e.stale = true,
            }
        }
    }

    /// MESSAGE_REACTION_REMOVE: the user leaves that list.
    pub fn on_remove(&mut self, message: MessageId, emoji: &Emoji, burst: bool, user: UserId) {
        let id = emoji_identity(emoji);
        for (k, e) in self.entries.iter_mut() {
            if k.message == message && k.emoji == id && k.burst == burst {
                let before = e.users.len();
                e.users.retain(|u| u.id != user);
                if e.users.len() == before && !e.complete {
                    // Someone we had not fetched left: counts shift.
                    e.stale = true;
                }
            }
        }
    }

    /// MESSAGE_REACTION_REMOVE_EMOJI (`emoji`) / REMOVE_ALL / message
    /// deleted (`None`).
    pub fn forget(&mut self, message: MessageId, emoji: Option<&Emoji>) {
        let id = emoji.map(emoji_identity);
        self.entries
            .retain(|k, _| k.message != message || id.as_ref().is_some_and(|i| *i != k.emoji));
    }

    fn evict(&mut self) {
        while self.entries.len() >= MAX_ENTRIES {
            let Some(oldest) = self
                .entries
                .iter()
                .filter(|(_, e)| !e.loading)
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| k.clone())
            else {
                return;
            };
            self.entries.remove(&oldest);
        }
    }
}

/// The open "Reactions" dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct ReactorsDialog {
    pub channel: ChannelId,
    pub message: MessageId,
    /// Selected tab: emoji + burst.
    pub emoji: Emoji,
    pub burst: bool,
}

/// Hover tooltips and the dialog.
#[derive(Debug, Default)]
pub struct ReactionsUi {
    pub cache: ReactorCache,
    /// The pill under the pointer.
    pub hovered: Option<ReactionKey>,
    /// Bumped on every hover change so stale delayed fetches do nothing.
    pub(super) hover_generation: u64,
    pub dialog: Option<ReactorsDialog>,
}

/// One tab of the dialog / one fetchable list of a message: its emoji,
/// kind and how many reacted that way.
pub fn tabs(reactions: &[Reaction]) -> Vec<(Emoji, bool, u32)> {
    let mut out = Vec::new();
    for r in reactions {
        let burst = r.burst_count.min(r.count);
        let normal = r.count - burst;
        if normal > 0 {
            out.push((r.emoji.clone(), false, normal));
        }
        if burst > 0 {
            out.push((r.emoji.clone(), true, burst));
        }
    }
    out
}

/// The list a pill's tooltip shows: normal reactors, or the burst ones
/// when the reaction only has burst reactions.
pub fn pill_kind(r: &Reaction) -> (bool, u32) {
    let burst = r.burst_count.min(r.count);
    if r.count > burst {
        (false, r.count - burst)
    } else {
        (true, burst)
    }
}

/// `:name:` for a custom emoji, the shortcode of a Unicode one.
pub fn emoji_label(e: &Emoji) -> String {
    if e.id.is_some() {
        return format!(":{}:", e.name);
    }
    // Discord's names are the word ones (`thumbsup`, not `+1`).
    let code = emojis::get(&e.name).and_then(|x| {
        x.shortcodes()
            .find(|s| s.starts_with(|c: char| c.is_ascii_alphabetic()))
            .or_else(|| x.shortcode())
    });
    match code {
        Some(s) => format!(":{s}:"),
        None => e.name.clone(),
    }
}

/// "Alice, Bob, and Carol reacted with :x:" / "…, and 5 others reacted
/// with :x:" (the official wording). `total` is the reaction count.
pub fn tooltip_text(names: &[&str], total: u32, emoji: &str) -> String {
    let shown = &names[..names.len().min(TOOLTIP_NAMES)];
    let others = (total as usize).saturating_sub(shown.len());
    let who = match (shown, others) {
        ([], _) => return format!("Reacted with {emoji}"),
        ([a], 0) => (*a).to_owned(),
        ([a, b], 0) => format!("{a} and {b}"),
        (list, 0) => {
            let (last, rest) = list.split_last().map_or((&"", list), |(l, r)| (l, r));
            format!("{}, and {last}", rest.join(", "))
        }
        (list, n) => format!(
            "{}, and {n} {}",
            list.join(", "),
            if n == 1 { "other" } else { "others" }
        ),
    };
    format!("{who} reacted with {emoji}")
}

impl App {
    pub(super) fn update_reactions(&mut self, u: Ui) -> Option<Task<Message>> {
        Some(match u {
            Ui::ReactionHover(key) => {
                let r = &mut self.reactions;
                if r.hovered.as_ref() == Some(&key) {
                    return Some(Task::none());
                }
                r.hovered = Some(key);
                r.hover_generation += 1;
                delayed(Done::ReactionHoverDue(r.hover_generation), HOVER_DELAY_MS)
            }
            Ui::ReactionUnhover(key) => {
                if self.reactions.hovered.as_ref() == Some(&key) {
                    self.reactions.hovered = None;
                    self.reactions.hover_generation += 1;
                }
                Task::none()
            }
            Ui::OpenReactors(message, emoji) => {
                let Some(channel) = self.open_channel() else {
                    return Some(Task::none());
                };
                let Some(m) = self.chat.message(channel, message) else {
                    return Some(Task::none());
                };
                let tabs = tabs(&m.reactions);
                let pick = emoji
                    .and_then(|e| tabs.iter().find(|(x, _, _)| *x == e))
                    .or(tabs.first())
                    .map(|(e, b, _)| (e.clone(), *b));
                let Some((emoji, burst)) = pick else {
                    return Some(Task::none());
                };
                self.close_overlays();
                self.reactions.dialog = Some(ReactorsDialog {
                    channel,
                    message,
                    emoji,
                    burst,
                });
                self.load_dialog_tab(false)
            }
            Ui::ReactorsTab(emoji, burst) => {
                if let Some(d) = self.reactions.dialog.as_mut() {
                    d.emoji = emoji;
                    d.burst = burst;
                }
                self.load_dialog_tab(false)
            }
            Ui::ReactorsMore => self.load_dialog_tab(true),
            Ui::CloseReactors => {
                self.reactions.dialog = None;
                Task::none()
            }
            _ => return None,
        })
    }

    /// The hover delay passed: fetch the hovered pill's reactors if the
    /// pointer is still there and they are not cached.
    pub(super) fn reaction_hover_due(&mut self, generation: u64) -> Task<Message> {
        if generation != self.reactions.hover_generation {
            return Task::none();
        }
        let Some(key) = self.reactions.hovered.clone() else {
            return Task::none();
        };
        let Some(count) = self.reaction_count(&key) else {
            return Task::none();
        };
        match self.reactions.cache.want(&key, TOOLTIP_LIMIT, count) {
            Some(f) => self.fetch_reactors(f),
            None => Task::none(),
        }
    }

    /// Fetches the selected dialog tab (`more`: the next page).
    fn load_dialog_tab(&mut self, more: bool) -> Task<Message> {
        let Some(d) = &self.reactions.dialog else {
            return Task::none();
        };
        let key = ReactionKey::new(d.channel, d.message, &d.emoji, d.burst);
        let Some(count) = self.reaction_count(&key) else {
            return Task::none();
        };
        let have = self
            .reactions
            .cache
            .get(&key)
            .map_or(0, |e| e.users.len() as u32);
        let wanted = if more {
            have + PAGE_LIMIT
        } else {
            have.max(PAGE_LIMIT)
        };
        match self.reactions.cache.want(&key, wanted, count) {
            Some(f) => self.fetch_reactors(f),
            None => Task::none(),
        }
    }

    /// How many reacted with this emoji and kind (`None`: the message or
    /// reaction is gone).
    fn reaction_count(&self, key: &ReactionKey) -> Option<u32> {
        let m = self.chat.message(key.channel, key.message)?;
        tabs(&m.reactions)
            .into_iter()
            .find(|(e, b, _)| emoji_identity(e) == key.emoji && *b == key.burst)
            .map(|(_, _, n)| n)
    }

    fn fetch_reactors(&mut self, f: Fetch) -> Task<Message> {
        let Some(emoji) = self
            .chat
            .message(f.key.channel, f.key.message)
            .and_then(|m| {
                m.reactions
                    .iter()
                    .find(|r| emoji_identity(&r.emoji) == f.key.emoji)
            })
            .map(|r| r.emoji.clone())
        else {
            self.reactions.cache.finish(&f, Err(()));
            return Task::none();
        };
        if self.cfg.mock {
            let users = crate::mock::reactors(&f.key, f.limit, f.after);
            self.reactions.cache.finish(&f, Ok(users));
            return Task::none();
        }
        let rest = self.rest.clone();
        Task::perform(
            {
                let f = f.clone();
                async move {
                    rest.reactions(
                        f.key.channel,
                        f.key.message,
                        &emoji,
                        f.key.burst,
                        f.limit,
                        f.after,
                    )
                    .await
                }
            },
            move |r| {
                Message::Done(Done::Reactors {
                    fetch: Box::new(f.clone()),
                    result: r.map_err(|e| e.user_message()),
                })
            },
        )
    }

    pub(super) fn reactors_done(
        &mut self,
        fetch: &Fetch,
        result: Result<Vec<User>, String>,
    ) -> Task<Message> {
        if let Err(e) = &result {
            tracing::debug!(error = %e, "reactors fetch failed");
        }
        self.reactions.cache.finish(fetch, result.map_err(|_| ()));
        Task::none()
    }

    /// Keeps the reactor cache in line with reaction events.
    pub(super) fn reactions_event(&mut self, ev: &ReactionEvent) {
        let c = &mut self.reactions.cache;
        match ev {
            ReactionEvent::Add {
                message_id,
                user_id,
                emoji,
                burst,
                ..
            } => {
                let user = self.chat.users.get(user_id);
                c.on_add(*message_id, emoji, *burst, user);
            }
            ReactionEvent::Remove {
                message_id,
                user_id,
                emoji,
                burst,
                ..
            } => c.on_remove(*message_id, emoji, *burst, *user_id),
            ReactionEvent::RemoveAll { message_id, .. } => c.forget(*message_id, None),
            ReactionEvent::RemoveEmoji {
                message_id, emoji, ..
            } => c.forget(*message_id, Some(emoji)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: u64) -> User {
        User {
            id: Snowflake(id),
            username: format!("u{id}"),
            ..Default::default()
        }
    }

    fn key(m: u64, e: &str) -> ReactionKey {
        ReactionKey::new(
            Snowflake(1),
            Snowflake(m),
            &Emoji {
                id: None,
                name: e.into(),
                animated: false,
            },
            false,
        )
    }

    fn emoji(e: &str) -> Emoji {
        Emoji {
            id: None,
            name: e.into(),
            animated: false,
        }
    }

    #[test]
    fn tooltip_wording() {
        assert_eq!(tooltip_text(&["A"], 1, ":x:"), "A reacted with :x:");
        assert_eq!(
            tooltip_text(&["A", "B"], 2, ":x:"),
            "A and B reacted with :x:"
        );
        assert_eq!(
            tooltip_text(&["A", "B", "C"], 3, ":x:"),
            "A, B, and C reacted with :x:"
        );
        assert_eq!(
            tooltip_text(&["A", "B", "C"], 4, ":x:"),
            "A, B, C, and 1 other reacted with :x:"
        );
        assert_eq!(
            tooltip_text(&["A", "B", "C"], 8, ":x:"),
            "A, B, C, and 5 others reacted with :x:"
        );
        assert_eq!(
            tooltip_text(&["A"], 3, ":x:"),
            "A, and 2 others reacted with :x:"
        );
        assert_eq!(tooltip_text(&[], 3, ":x:"), "Reacted with :x:");
    }

    #[test]
    fn labels() {
        assert_eq!(emoji_label(&emoji("\u{1F44D}")), ":thumbsup:");
        assert_eq!(
            emoji_label(&Emoji {
                id: Some(Snowflake(9)),
                name: "ferris".into(),
                animated: false
            }),
            ":ferris:"
        );
    }

    #[test]
    fn fetches_once_and_caches() {
        let mut c = ReactorCache::default();
        let k = key(10, "a");
        let f = c.want(&k, TOOLTIP_LIMIT, 7).expect("first hover fetches");
        assert_eq!((f.limit, f.after), (3, None));
        // In flight: no second request.
        assert!(c.want(&k, TOOLTIP_LIMIT, 7).is_none());
        c.finish(&f, Ok(vec![user(1), user(2), user(3)]));
        let e = c.get(&k).unwrap();
        assert!(!e.complete && e.users.len() == 3);
        // Cached: the next hover does not fetch.
        assert!(c.want(&k, TOOLTIP_LIMIT, 7).is_none());
        // The dialog wants a full page: continues after the last user.
        let f = c.want(&k, PAGE_LIMIT, 7).expect("dialog fetches more");
        assert_eq!((f.limit, f.after), (PAGE_LIMIT, Some(Snowflake(3))));
        c.finish(&f, Ok(vec![user(4), user(5)]));
        assert!(c.get(&k).unwrap().complete);
        assert_eq!(c.get(&k).unwrap().users.len(), 5);
        assert!(c.want(&k, PAGE_LIMIT, 7).is_none());
    }

    #[test]
    fn failures_retry_and_small_counts_are_capped() {
        let mut c = ReactorCache::default();
        let k = key(10, "a");
        let f = c.want(&k, TOOLTIP_LIMIT, 1).unwrap();
        assert_eq!(f.limit, 1);
        c.finish(&f, Err(()));
        assert!(c.get(&k).unwrap().failed);
        let f = c.want(&k, TOOLTIP_LIMIT, 1).expect("retried on next hover");
        c.finish(&f, Ok(vec![user(1)]));
        assert!(c.want(&k, TOOLTIP_LIMIT, 1).is_none());
    }

    #[test]
    fn events_patch_or_mark_stale() {
        let mut c = ReactorCache::default();
        let k = key(10, "a");
        let f = c.want(&k, TOOLTIP_LIMIT, 2).unwrap();
        c.finish(&f, Ok(vec![user(1), user(5)]));
        assert!(c.get(&k).unwrap().complete);
        // A known user joins a complete list in id order.
        c.on_add(Snowflake(10), &emoji("a"), false, Some(&user(3)));
        let ids: Vec<u64> = c.get(&k).unwrap().users.iter().map(|u| u.id.0).collect();
        assert_eq!(ids, vec![1, 3, 5]);
        assert!(c.want(&k, TOOLTIP_LIMIT, 3).is_none());
        c.on_remove(Snowflake(10), &emoji("a"), false, Snowflake(1));
        assert_eq!(c.get(&k).unwrap().users.len(), 2);
        // Another emoji / message is untouched.
        c.on_add(Snowflake(11), &emoji("a"), false, None);
        c.on_add(Snowflake(10), &emoji("b"), false, None);
        c.on_add(Snowflake(10), &emoji("a"), true, None);
        assert!(!c.get(&k).unwrap().stale);
        // An unknown user: refetch on next use, from the start.
        c.on_add(Snowflake(10), &emoji("a"), false, None);
        assert!(c.get(&k).unwrap().stale);
        let f = c.want(&k, TOOLTIP_LIMIT, 3).expect("stale list refetches");
        assert_eq!(f.after, None);
        c.finish(&f, Ok(vec![user(2), user(3), user(5)]));
        assert!(!c.get(&k).unwrap().stale);
        // Removing the emoji or all reactions drops the lists.
        c.forget(Snowflake(10), Some(&emoji("b")));
        assert!(c.get(&k).is_some());
        c.forget(Snowflake(10), Some(&emoji("a")));
        assert!(c.get(&k).is_none());
    }

    #[test]
    fn cache_is_bounded() {
        let mut c = ReactorCache::default();
        for m in 0..(MAX_ENTRIES as u64 * 3) {
            let k = key(m, "a");
            let f = c.want(&k, TOOLTIP_LIMIT, 3).unwrap();
            c.finish(&f, Ok(vec![user(1)]));
        }
        assert!(c.len() <= MAX_ENTRIES);
        // The most recent stays.
        assert!(c.get(&key(MAX_ENTRIES as u64 * 3 - 1, "a")).is_some());
    }

    #[test]
    fn burst_reactions_get_their_own_tab() {
        let reactions = vec![
            Reaction {
                emoji: emoji("a"),
                count: 5,
                me: false,
                burst_count: 2,
            },
            Reaction {
                emoji: emoji("b"),
                count: 1,
                me: true,
                burst_count: 1,
            },
        ];
        assert_eq!(
            tabs(&reactions),
            vec![
                (emoji("a"), false, 3),
                (emoji("a"), true, 2),
                (emoji("b"), true, 1)
            ]
        );
        assert_eq!(pill_kind(&reactions[1]), (true, 1));
        assert_eq!(pill_kind(&reactions[0]), (false, 3));
    }
}
