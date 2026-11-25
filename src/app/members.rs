//! Member list state, driven by GUILD_MEMBER_LIST_UPDATE operations.
//!
//! The list is sparse: only the index ranges we subscribed to are loaded.
//! It is bounded by [`MAX_ROWS`] so a huge server cannot grow memory.

use crate::net::model::*;

/// Rows (groups + members) kept at most.
pub const MAX_ROWS: usize = 1500;
/// Members requested per subscription range.
pub const RANGE_SIZE: usize = 100;
/// Ranges requested at most (so at most `RANGE_SIZE * MAX_RANGES` rows).
pub const MAX_RANGES: usize = 5;

#[derive(Debug, Clone, Default)]
pub struct MemberListState {
    pub guild: GuildId,
    pub list_id: String,
    pub member_count: u32,
    pub online_count: u32,
    /// Flat list of group headers and members; `None` = not loaded.
    pub rows: Vec<Option<MemberItem>>,
}

impl MemberListState {
    pub fn new(guild: GuildId) -> Self {
        Self {
            guild,
            ..Default::default()
        }
    }

    /// Applies one update. Updates for another guild are ignored.
    pub fn apply(&mut self, u: &MemberListUpdate) {
        if u.guild_id != self.guild {
            return;
        }
        self.list_id.clone_from(&u.list_id);
        self.member_count = u.member_count;
        self.online_count = u.online_count;
        for op in &u.ops {
            match op {
                MemberOp::Sync { start, items } => {
                    let end = start.saturating_add(items.len()).min(MAX_ROWS);
                    if *start >= MAX_ROWS {
                        continue;
                    }
                    if self.rows.len() < end {
                        self.rows.resize(end, None);
                    }
                    for (i, item) in items.iter().take(end - start).enumerate() {
                        self.rows[start + i] = Some(item.clone());
                    }
                }
                MemberOp::Insert { index, item } => {
                    if *index <= self.rows.len() && self.rows.len() < MAX_ROWS {
                        self.rows.insert(*index, Some(item.clone()));
                    }
                }
                MemberOp::Update { index, item } => {
                    if *index < MAX_ROWS {
                        if self.rows.len() <= *index {
                            self.rows.resize(*index + 1, None);
                        }
                        self.rows[*index] = Some(item.clone());
                    }
                }
                MemberOp::Delete { index } => {
                    if *index < self.rows.len() {
                        self.rows.remove(*index);
                    }
                }
                MemberOp::Invalidate { start, end } => {
                    for i in *start..=(*end).min(self.rows.len().saturating_sub(1)) {
                        if let Some(r) = self.rows.get_mut(i) {
                            *r = None;
                        }
                    }
                }
            }
        }
        // The server tells the exact length of the list through its groups.
        if !u.groups.is_empty() {
            let total = u
                .groups
                .iter()
                .map(|(_, n)| *n as usize)
                .sum::<usize>()
                .saturating_add(u.groups.len())
                .min(MAX_ROWS);
            self.rows.resize(total, None);
        }
    }

    /// Loaded rows in order (gaps skipped).
    pub fn loaded(&self) -> impl Iterator<Item = &MemberItem> {
        self.rows.iter().flatten()
    }

    /// Index one past the last loaded row.
    pub fn loaded_end(&self) -> usize {
        self.rows
            .iter()
            .rposition(Option::is_some)
            .map_or(0, |i| i + 1)
    }

    /// Whether rows exist beyond what is loaded.
    pub fn has_more(&self) -> bool {
        self.rows.len() > self.loaded_end()
    }

    /// Ranges to subscribe to so that `rows_wanted` rows are covered.
    pub fn ranges_for(rows_wanted: usize) -> Vec<(usize, usize)> {
        let n = rows_wanted.div_ceil(RANGE_SIZE).clamp(1, MAX_RANGES);
        (0..n)
            .map(|i| (i * RANGE_SIZE, (i + 1) * RANGE_SIZE - 1))
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
        MemberListUpdate {
            guild_id: Snowflake(1),
            list_id: "everyone".into(),
            member_count: 5,
            online_count: 2,
            groups: groups.into_iter().map(|(a, b)| (a.into(), b)).collect(),
            ops,
        }
    }

    #[test]
    fn sync_fills_and_sizes_the_list() {
        let mut l = MemberListState::new(Snowflake(1));
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
        assert_eq!(l.rows.len(), 7);
        assert_eq!(l.loaded().count(), 4);
        assert_eq!(l.loaded_end(), 4);
        assert_eq!(l.member_count, 5);
    }

    #[test]
    fn insert_update_delete_invalidate() {
        let mut l = MemberListState::new(Snowflake(1));
        l.apply(&upd(
            vec![MemberOp::Sync {
                start: 0,
                items: vec![group("online", 1), member(10, Presence::Online)],
            }],
            vec![],
        ));
        l.apply(&upd(
            vec![MemberOp::Insert {
                index: 1,
                item: member(11, Presence::Idle),
            }],
            vec![],
        ));
        assert_eq!(l.rows.len(), 3);
        assert!(matches!(&l.rows[1], Some(MemberItem::Member(m)) if m.user.id == Snowflake(11)));
        l.apply(&upd(
            vec![MemberOp::Update {
                index: 2,
                item: member(10, Presence::Dnd),
            }],
            vec![],
        ));
        assert!(matches!(&l.rows[2], Some(MemberItem::Member(m)) if m.presence == Presence::Dnd));
        l.apply(&upd(vec![MemberOp::Delete { index: 1 }], vec![]));
        assert_eq!(l.rows.len(), 2);
        l.apply(&upd(
            vec![MemberOp::Invalidate { start: 0, end: 5 }],
            vec![],
        ));
        assert_eq!(l.loaded().count(), 0);
        // Out-of-range ops never panic.
        l.apply(&upd(
            vec![
                MemberOp::Delete { index: 99 },
                MemberOp::Insert {
                    index: 99,
                    item: group("x", 0),
                },
            ],
            vec![],
        ));
    }

    #[test]
    fn other_guild_is_ignored_and_size_is_capped() {
        let mut l = MemberListState::new(Snowflake(2));
        l.apply(&upd(
            vec![MemberOp::Sync {
                start: 0,
                items: vec![group("online", 1)],
            }],
            vec![],
        ));
        assert!(l.rows.is_empty());
        let mut l = MemberListState::new(Snowflake(1));
        l.apply(&upd(
            vec![MemberOp::Sync {
                start: MAX_ROWS + 10,
                items: vec![group("online", 1)],
            }],
            vec![("online", 1_000_000)],
        ));
        assert_eq!(l.rows.len(), MAX_ROWS);
    }

    #[test]
    fn ranges_are_bounded() {
        assert_eq!(MemberListState::ranges_for(0), vec![(0, 99)]);
        assert_eq!(MemberListState::ranges_for(150), vec![(0, 99), (100, 199)]);
        assert_eq!(MemberListState::ranges_for(100_000).len(), MAX_RANGES);
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
                hoist: false,
                mentionable: false,
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
