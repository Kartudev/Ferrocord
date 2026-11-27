//! Member list state, driven by GUILD_MEMBER_LIST_UPDATE operations.
//!
//! Like the official client, the sidebar subscribes (gateway op 14) to the
//! first 100 rows plus the 100-row chunks around the visible window, and
//! the server sends SYNC / INSERT / UPDATE / DELETE / INVALIDATE operations
//! for those ranges. The list knows its full length from the `groups`
//! counts, but keeps only rows inside the subscribed ranges, so memory is
//! bounded however large the server is (offline members are left out by
//! Discord itself above ~1000 members).
//!
//! A list belongs to a channel's *list id* (`everyone`, or a hash of the
//! channel's view permissions): updates for another list are ignored, and
//! after a channel switch the first SYNC of the new list replaces the rows.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::net::model::*;

/// Members requested per subscription range.
pub const RANGE_SIZE: usize = 100;
/// Ranges sent per channel at most (the official client sends three:
/// `[0, 99]` and the two chunks at the visible window).
pub const MAX_RANGES: usize = 3;
/// Rows kept at most, whatever the server sends (safety net: the ranges
/// already bound it to `MAX_RANGES * RANGE_SIZE`).
pub const MAX_KEPT: usize = 1000;
/// Rows below the visible window that already count as "visible" when
/// picking ranges, so the next chunk is subscribed before it scrolls in.
pub const PRELOAD_ROWS: usize = 30;

#[derive(Debug, Clone, Default)]
pub struct MemberListState {
    pub guild: GuildId,
    pub channel: ChannelId,
    /// The list the rows belong to (adopted from a SYNC).
    pub list_id: Option<String>,
    /// The channel changed: the next SYNC may be for another list.
    awaiting_sync: bool,
    pub member_count: u32,
    pub online_count: u32,
    /// `(group id, member count)` in list order, from the last update.
    pub groups: Vec<(String, u32)>,
    /// Total rows (group headers + members) of the server's list.
    len: usize,
    /// Loaded rows by index (only inside the subscribed ranges).
    items: BTreeMap<usize, MemberItem>,
    /// Ranges currently subscribed (rows outside are dropped).
    ranges: Vec<(usize, usize)>,
}

impl MemberListState {
    pub fn new(guild: GuildId, channel: ChannelId) -> Self {
        Self {
            guild,
            channel,
            ranges: vec![(0, RANGE_SIZE - 1)],
            ..Default::default()
        }
    }

    /// The open channel changed within the same server. Rows stay on
    /// screen until the new list's SYNC arrives (the list is often the
    /// same one: `everyone`).
    pub fn switch_channel(&mut self, channel: ChannelId) {
        if channel != self.channel {
            self.channel = channel;
            self.awaiting_sync = true;
        }
    }

    /// Total rows of the list (loaded or not).
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The row at `index`, if loaded.
    pub fn get(&self, index: usize) -> Option<&MemberItem> {
        self.items.get(&index)
    }

    /// Number of loaded rows.
    pub fn loaded_count(&self) -> usize {
        self.items.len()
    }

    /// Loaded rows in order.
    pub fn loaded(&self) -> impl Iterator<Item = &MemberItem> {
        self.items.values()
    }

    /// Loaded rows inside `rows`.
    pub fn loaded_in(&self, rows: Range<usize>) -> impl Iterator<Item = &MemberItem> {
        self.items.range(rows).map(|(_, v)| v)
    }

    /// Applies one update. Updates for another guild, or for another list
    /// than the one shown, are ignored.
    pub fn apply(&mut self, u: &MemberListUpdate) {
        if u.guild_id != self.guild {
            return;
        }
        let has_sync = u.ops.iter().any(|o| matches!(o, MemberOp::Sync { .. }));
        match &self.list_id {
            Some(id) if *id == u.list_id => {
                if has_sync {
                    self.awaiting_sync = false;
                }
            }
            // The first SYNC after a channel switch: another list.
            Some(_) if has_sync && self.awaiting_sync => {
                self.items.clear();
                self.list_id = Some(u.list_id.clone());
                self.awaiting_sync = false;
            }
            // A list we are not showing (the previous channel's).
            Some(_) => return,
            None if has_sync => {
                self.list_id = Some(u.list_id.clone());
                self.awaiting_sync = false;
            }
            // Operations on rows we never synced cannot be placed.
            None => return,
        }
        self.member_count = u.member_count;
        self.online_count = u.online_count;
        for op in &u.ops {
            match op {
                MemberOp::Sync { start, items } => {
                    for (i, item) in items.iter().enumerate() {
                        self.items.insert(start.saturating_add(i), item.clone());
                    }
                    self.len = self.len.max(start.saturating_add(items.len()));
                }
                MemberOp::Insert { index, item } => {
                    let tail = self.items.split_off(index);
                    self.items
                        .extend(tail.into_iter().map(|(i, v)| (i.saturating_add(1), v)));
                    self.items.insert(*index, item.clone());
                    self.len = self.len.saturating_add(1);
                }
                MemberOp::Update { index, item } => {
                    self.items.insert(*index, item.clone());
                }
                MemberOp::Delete { index } => {
                    self.items.remove(index);
                    let tail = self.items.split_off(index);
                    self.items
                        .extend(tail.into_iter().map(|(i, v)| (i.saturating_sub(1), v)));
                    self.len = self.len.saturating_sub(1);
                }
                MemberOp::Invalidate { start, end } => {
                    if start <= end {
                        let mut tail = self.items.split_off(start);
                        let mut after = tail.split_off(&end.saturating_add(1));
                        tail.clear();
                        self.items.append(&mut after);
                    }
                }
            }
        }
        // The groups give the exact length of the list (every update
        // carries them; an empty list has none).
        self.groups.clone_from(&u.groups);
        if !u.groups.is_empty() || u.ops.iter().any(|o| matches!(o, MemberOp::Sync { .. })) {
            self.len = u
                .groups
                .iter()
                .map(|(_, n)| *n as usize)
                .sum::<usize>()
                .saturating_add(u.groups.len());
        }
        let len = self.len;
        self.items.retain(|i, _| *i < len);
        self.prune();
    }

    /// Records the subscribed ranges and drops rows outside them.
    pub fn set_ranges(&mut self, ranges: &[(usize, usize)]) {
        ranges.clone_into(&mut self.ranges);
        self.prune();
    }

    fn prune(&mut self) {
        let ranges = &self.ranges;
        self.items
            .retain(|i, _| ranges.iter().any(|(a, b)| a <= i && i <= b));
        while self.items.len() > MAX_KEPT {
            self.items.pop_last();
        }
    }

    /// Indexes of the group header rows, from the group counts (or the
    /// loaded headers when the server sent no groups).
    pub fn header_rows(&self) -> Vec<usize> {
        if self.groups.is_empty() {
            return self
                .items
                .iter()
                .filter(|(_, v)| matches!(v, MemberItem::Group { .. }))
                .map(|(i, _)| *i)
                .collect();
        }
        let mut out = Vec::with_capacity(self.groups.len());
        let mut at = 0usize;
        for (_, n) in &self.groups {
            out.push(at);
            at = at.saturating_add(*n as usize).saturating_add(1);
        }
        out
    }

    /// Ranges to subscribe to for the visible rows `first..=last`: the
    /// first chunk always, then the chunks at the window (with a preload
    /// margin), at most [`MAX_RANGES`].
    pub fn ranges_for_window(first: usize, last: usize) -> Vec<(usize, usize)> {
        let chunk = |row: usize| row / RANGE_SIZE;
        let a = chunk(first);
        let b = chunk(last.max(first).saturating_add(PRELOAD_ROWS)).max(a);
        let mut chunks = vec![0usize];
        for c in a..=b {
            if chunks.len() >= MAX_RANGES {
                break;
            }
            if !chunks.contains(&c) {
                chunks.push(c);
            }
        }
        chunks
            .into_iter()
            .map(|c| (c * RANGE_SIZE, c * RANGE_SIZE + RANGE_SIZE - 1))
            .collect()
    }

    /// Display name of a group header.
    pub fn group_label(id: &str, guild: Option<&Guild>) -> String {
        match id {
            "online" => "Online".into(),
            "offline" => "Offline".into(),
            other => other
                .parse::<u64>()
                .ok()
                .and_then(|r| guild?.roles.iter().find(|x| x.id.0 == r))
                .map(|r| r.name.clone())
                .unwrap_or_else(|| "Members".into()),
        }
    }
}

/// Fixed row heights of the sidebar, so the visible window can be computed
/// from the scroll offset without laying out the whole list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowMetrics {
    pub top_pad: f32,
    pub member: f32,
    pub header: f32,
}

impl RowMetrics {
    /// Top edge of row `index` (`headers` sorted).
    pub fn offset(&self, headers: &[usize], index: usize) -> f32 {
        let h = headers.partition_point(|&x| x < index);
        self.top_pad + (index - h) as f32 * self.member + h as f32 * self.header
    }

    /// Height of all `len` rows plus the top padding.
    pub fn total(&self, headers: &[usize], len: usize) -> f32 {
        self.offset(headers, len)
    }

    /// The row at vertical position `y` (clamped to the list).
    pub fn row_at(&self, headers: &[usize], len: usize, y: f32) -> usize {
        if len == 0 {
            return 0;
        }
        // Binary search on the monotonic offset.
        let (mut lo, mut hi) = (0usize, len);
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if self.offset(headers, mid) <= y {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// Rows intersecting `[y, y + height)`, plus `overscan` rows each side.
    pub fn window(
        &self,
        headers: &[usize],
        len: usize,
        y: f32,
        height: f32,
        overscan: usize,
    ) -> Range<usize> {
        if len == 0 {
            return 0..0;
        }
        let first = self.row_at(headers, len, y.max(0.0));
        let last = self.row_at(headers, len, (y + height.max(0.0)).max(0.0));
        first.saturating_sub(overscan)..(last + 1 + overscan).min(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: u64, p: Presence) -> MemberItem {
        MemberItem::Member(Box::new(MemberEntry {
            user: User {
                id: Snowflake(id),
                username: format!("u{id}"),
                global_name: None,
                avatar: None,
                bot: false,
                ..Default::default()
            },
            nick: None,
            roles: vec![],
            presence: p,
            activities: vec![],
        }))
    }

    fn group(id: &str, n: u32) -> MemberItem {
        MemberItem::Group {
            id: id.into(),
            count: n,
        }
    }

    fn upd(ops: Vec<MemberOp>, groups: Vec<(&str, u32)>) -> MemberListUpdate {
        upd_list("everyone", ops, groups)
    }

    fn upd_list(list: &str, ops: Vec<MemberOp>, groups: Vec<(&str, u32)>) -> MemberListUpdate {
        MemberListUpdate {
            guild_id: Snowflake(1),
            list_id: list.into(),
            member_count: 5,
            online_count: 2,
            groups: groups.into_iter().map(|(a, b)| (a.into(), b)).collect(),
            ops,
        }
    }

    fn ids(l: &MemberListState) -> Vec<String> {
        (0..l.len())
            .map(|i| match l.get(i) {
                Some(MemberItem::Member(m)) => m.user.id.0.to_string(),
                Some(MemberItem::Group { id, .. }) => format!("[{id}]"),
                None => "_".into(),
            })
            .collect()
    }

    fn new() -> MemberListState {
        MemberListState::new(Snowflake(1), Snowflake(100))
    }

    #[test]
    fn sync_fills_and_sizes_the_list() {
        let mut l = new();
        l.apply(&upd(
            vec![MemberOp::Sync {
                start: 0,
                items: vec![
                    group("online", 2),
                    member(10, Presence::Online),
                    member(11, Presence::Dnd),
                    group("offline", 3),
                ],
            }],
            vec![("online", 2), ("offline", 3)],
        ));
        // 2 groups + 5 members.
        assert_eq!(l.len(), 7);
        assert_eq!(l.loaded_count(), 4);
        assert_eq!(l.member_count, 5);
        assert_eq!(l.header_rows(), vec![0, 3]);
    }

    #[test]
    fn insert_update_delete_invalidate_shift_indexes() {
        let mut l = new();
        l.apply(&upd(
            vec![MemberOp::Sync {
                start: 0,
                items: vec![
                    group("online", 3),
                    member(10, Presence::Online),
                    member(12, Presence::Online),
                    member(13, Presence::Online),
                ],
            }],
            vec![("online", 3)],
        ));
        l.apply(&upd(
            vec![MemberOp::Insert {
                index: 2,
                item: member(11, Presence::Idle),
            }],
            vec![("online", 4)],
        ));
        assert_eq!(ids(&l), ["[online]", "10", "11", "12", "13"]);
        l.apply(&upd(
            vec![MemberOp::Update {
                index: 4,
                item: member(13, Presence::Dnd),
            }],
            vec![("online", 4)],
        ));
        assert!(matches!(l.get(4), Some(MemberItem::Member(m)) if m.presence == Presence::Dnd));
        l.apply(&upd(
            vec![MemberOp::Delete { index: 1 }],
            vec![("online", 3)],
        ));
        assert_eq!(ids(&l), ["[online]", "11", "12", "13"]);
        // Moving a member (official: DELETE then INSERT in one update).
        l.apply(&upd(
            vec![
                MemberOp::Delete { index: 3 },
                MemberOp::Insert {
                    index: 1,
                    item: member(13, Presence::Online),
                },
            ],
            vec![("online", 3)],
        ));
        assert_eq!(ids(&l), ["[online]", "13", "11", "12"]);
        l.apply(&upd(
            vec![MemberOp::Invalidate { start: 1, end: 2 }],
            vec![("online", 3)],
        ));
        assert_eq!(ids(&l), ["[online]", "_", "_", "12"]);
        // Out-of-range or reversed ops never panic.
        l.apply(&upd(
            vec![
                MemberOp::Delete { index: 99 },
                MemberOp::Invalidate {
                    start: 5,
                    end: usize::MAX,
                },
                MemberOp::Invalidate { start: 3, end: 1 },
                MemberOp::Insert {
                    index: usize::MAX,
                    item: group("x", 0),
                },
            ],
            vec![("online", 3)],
        ));
        assert_eq!(l.len(), 4);
    }

    /// Regression: past the old 1500-row cap an INSERT was dropped and every
    /// later row was shown one place off.
    #[test]
    fn inserts_far_down_a_large_list_keep_rows_aligned() {
        let mut l = new();
        l.set_ranges(&[(0, 99), (2000, 2099)]);
        let items: Vec<MemberItem> = (0..100)
            .map(|i| member(5000 + i, Presence::Online))
            .collect();
        l.apply(&upd(
            vec![MemberOp::Sync { start: 2000, items }],
            vec![("online", 4999)],
        ));
        assert_eq!(l.len(), 5000);
        l.apply(&upd(
            vec![MemberOp::Insert {
                index: 1000,
                item: member(1, Presence::Online),
            }],
            vec![("online", 5000)],
        ));
        // Row 2001 is now what was row 2000; the inserted row is outside
        // the subscribed ranges and not kept.
        assert!(matches!(l.get(2001), Some(MemberItem::Member(m)) if m.user.id.0 == 5000));
        assert!(l.get(1000).is_none());
        assert_eq!(l.len(), 5001);
    }

    /// Regression: switching channels in one server mixed the old list's
    /// rows (and its later updates) into the new one.
    #[test]
    fn channel_switch_takes_the_new_list_and_ignores_the_old() {
        let mut l = new();
        l.apply(&upd(
            vec![MemberOp::Sync {
                start: 0,
                items: vec![
                    group("online", 3),
                    member(10, Presence::Online),
                    member(11, Presence::Online),
                    member(12, Presence::Online),
                ],
            }],
            vec![("online", 3)],
        ));
        l.switch_channel(Snowflake(200));
        // An old-list update arriving late is still applied (same list).
        l.apply(&upd(
            vec![MemberOp::Delete { index: 3 }],
            vec![("online", 2)],
        ));
        assert_eq!(ids(&l), ["[online]", "10", "11"]);
        // The new channel's list (private channel: another id).
        l.apply(&upd_list(
            "123",
            vec![MemberOp::Sync {
                start: 0,
                items: vec![group("online", 1), member(20, Presence::Online)],
            }],
            vec![("online", 1)],
        ));
        assert_eq!(ids(&l), ["[online]", "20"]);
        assert_eq!(l.list_id.as_deref(), Some("123"));
        // Later updates of the old list are ignored.
        l.apply(&upd(
            vec![MemberOp::Insert {
                index: 1,
                item: member(13, Presence::Online),
            }],
            vec![("online", 4)],
        ));
        assert_eq!(ids(&l), ["[online]", "20"]);
        // Even a SYNC of the old list (no switch pending).
        l.apply(&upd(
            vec![MemberOp::Sync {
                start: 0,
                items: vec![group("online", 1), member(10, Presence::Online)],
            }],
            vec![("online", 1)],
        ));
        assert_eq!(ids(&l), ["[online]", "20"]);
    }

    #[test]
    fn updates_before_any_sync_or_for_another_guild_are_ignored() {
        let mut l = MemberListState::new(Snowflake(2), Snowflake(100));
        l.apply(&upd(
            vec![MemberOp::Sync {
                start: 0,
                items: vec![group("online", 1)],
            }],
            vec![("online", 1)],
        ));
        assert!(l.is_empty());
        let mut l = new();
        l.apply(&upd(
            vec![MemberOp::Insert {
                index: 0,
                item: member(1, Presence::Online),
            }],
            vec![("online", 1)],
        ));
        assert!(l.is_empty());
    }

    #[test]
    fn rows_outside_the_ranges_are_dropped() {
        let mut l = new();
        l.set_ranges(&[(0, 99), (100, 199)]);
        let items: Vec<MemberItem> = (0..200).map(|i| member(i, Presence::Online)).collect();
        l.apply(&upd(
            vec![MemberOp::Sync { start: 0, items }],
            vec![("online", 100_000)],
        ));
        assert_eq!(l.loaded_count(), 200);
        assert_eq!(l.len(), 100_001);
        l.set_ranges(&[(0, 99), (500, 599)]);
        assert_eq!(l.loaded_count(), 100);
        // A SYNC for a range we never asked for is not kept either.
        let items: Vec<MemberItem> = (0..2000).map(|i| member(i, Presence::Online)).collect();
        l.apply(&upd(
            vec![MemberOp::Sync { start: 0, items }],
            vec![("online", 100_000)],
        ));
        assert_eq!(l.loaded_count(), 200);
    }

    #[test]
    fn ranges_follow_the_window() {
        assert_eq!(MemberListState::ranges_for_window(0, 20), vec![(0, 99)]);
        assert_eq!(
            MemberListState::ranges_for_window(60, 80),
            vec![(0, 99), (100, 199)]
        );
        assert_eq!(
            MemberListState::ranges_for_window(250, 260),
            vec![(0, 99), (200, 299)]
        );
        assert_eq!(
            MemberListState::ranges_for_window(280, 300),
            vec![(0, 99), (200, 299), (300, 399)]
        );
        // Deep in a huge list: still three ranges.
        let r = MemberListState::ranges_for_window(99_950, 100_100);
        assert_eq!(r.len(), MAX_RANGES);
        assert_eq!(r[0], (0, 99));
        assert_eq!(r[1], (99_900, 99_999));
    }

    #[test]
    fn row_metrics_window() {
        let m = RowMetrics {
            top_pad: 16.0,
            member: 44.0,
            header: 40.0,
        };
        let headers = vec![0, 3, 10];
        assert_eq!(m.offset(&headers, 0), 16.0);
        assert_eq!(m.offset(&headers, 1), 56.0);
        assert_eq!(m.offset(&headers, 4), 16.0 + 2.0 * 40.0 + 2.0 * 44.0);
        assert_eq!(m.row_at(&headers, 20, 0.0), 0);
        assert_eq!(m.row_at(&headers, 20, 57.0), 1);
        assert_eq!(m.row_at(&headers, 20, 1e9), 19);
        assert_eq!(m.window(&headers, 20, 57.0, 44.0, 0), 1..3);
        assert_eq!(m.window(&headers, 0, 0.0, 500.0, 5), 0..0);
        assert_eq!(m.total(&headers, 20), m.offset(&headers, 20));
    }

    #[test]
    fn group_labels() {
        let g = Guild {
            id: Snowflake(1),
            name: "g".into(),
            icon: None,
            roles: vec![Role {
                id: Snowflake(7),
                name: "Moderators".into(),
                color: 0,
                position: 1,
                permissions: 0,
            }],
            owner_id: None,
            emojis: vec![],
            stickers: vec![],
            premium_tier: 0,
        };
        assert_eq!(MemberListState::group_label("online", None), "Online");
        assert_eq!(MemberListState::group_label("7", Some(&g)), "Moderators");
        assert_eq!(MemberListState::group_label("8", Some(&g)), "Members");
    }
}
