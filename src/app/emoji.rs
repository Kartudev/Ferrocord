//! Emoji picker data: Unicode emoji by category (from the `emojis` crate)
//! plus the current server's custom emoji. Pure functions, unit-tested.

use emojis::Group;

use crate::app::nitro::{Access, Usable};
use crate::net::model::*;

/// Unicode categories in the order of the official picker.
pub const CATEGORIES: [(Group, &str); 9] = [
    (Group::SmileysAndEmotion, "People"),
    (Group::PeopleAndBody, "Gestures"),
    (Group::AnimalsAndNature, "Nature"),
    (Group::FoodAndDrink, "Food"),
    (Group::Activities, "Activities"),
    (Group::TravelAndPlaces, "Travel"),
    (Group::Objects, "Objects"),
    (Group::Symbols, "Symbols"),
    (Group::Flags, "Flags"),
];

/// Most results shown for a search.
pub const MAX_RESULTS: usize = 120;

/// `meta` key of the recently used emoji (a `pref.` key: kept on logout).
pub const RECENT_KEY: &str = "pref.recent_emoji";

/// Most recently used emoji kept: four rows of the picker.
pub const RECENT_MAX: usize = 36;

/// Recent emoji offered as one-click reactions on a message's hover bar,
/// like the official client's.
pub const QUICK_REACTIONS: usize = 3;

/// Where a picked emoji goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerTarget {
    /// Insert into the composer.
    Composer,
    /// React to a message.
    Reaction {
        channel: ChannelId,
        message: MessageId,
    },
}

/// Which list the picker shows when the search box is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// The recently used emoji ([`Recent`]).
    Recent,
    /// Custom emoji of this server.
    Server(GuildId),
    /// Index into [`CATEGORIES`].
    Unicode(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmojiPicker {
    pub target: PickerTarget,
    pub query: String,
    pub section: Section,
    /// A custom emoji picked that needs Nitro here: the picker shows the
    /// lock footer with "Send as image" instead of using it.
    pub locked: Option<Emoji>,
}

impl EmojiPicker {
    /// Opens on the recently used emoji when there are any, else on the
    /// open server's custom emoji when it has any, else people.
    pub fn open(target: PickerTarget, guild: Option<&Guild>, has_recent: bool) -> Self {
        let section = match guild {
            _ if has_recent => Section::Recent,
            Some(g) if g.emojis.iter().any(|e| e.available) => Section::Server(g.id),
            _ => Section::Unicode(0),
        };
        Self {
            target,
            query: String::new(),
            section,
            locked: None,
        }
    }
}

/// Unicode emoji of a category, without skin-tone variants.
pub fn category(index: usize) -> Vec<&'static emojis::Emoji> {
    let Some((g, _)) = CATEGORIES.get(index) else {
        return vec![];
    };
    g.emojis()
        .filter(|e| e.skin_tone().is_none_or(|t| t == emojis::SkinTone::Default))
        .collect()
}

/// Case-insensitive search over names and shortcodes. Prefix matches rank
/// first. Variants with a skin tone are skipped.
pub fn search(query: &str) -> Vec<&'static emojis::Emoji> {
    let q = query.trim().trim_matches(':').to_lowercase();
    if q.is_empty() {
        return vec![];
    }
    let mut starts = Vec::new();
    let mut contains = Vec::new();
    for e in emojis::iter() {
        if e.skin_tone()
            .is_some_and(|t| t != emojis::SkinTone::Default)
        {
            continue;
        }
        let name = e.name().to_lowercase();
        let codes: Vec<&str> = e.shortcodes().collect();
        if name.starts_with(&q) || codes.iter().any(|c| c.starts_with(&q)) {
            starts.push(e);
        } else if name.contains(&q) || codes.iter().any(|c| c.contains(&q)) {
            contains.push(e);
        }
        if starts.len() >= MAX_RESULTS {
            break;
        }
    }
    starts.extend(contains);
    starts.truncate(MAX_RESULTS);
    starts
}

/// Custom emoji matching `query` (all of them when empty).
pub fn custom_matching<'a>(guild: &'a Guild, query: &str) -> Vec<&'a CustomEmoji> {
    let q = query.trim().trim_matches(':').to_lowercase();
    guild
        .emojis
        .iter()
        .filter(|e| e.available && (q.is_empty() || e.name.to_lowercase().contains(&q)))
        .collect()
}

/// Servers with emoji the user can see, the open one first, then the
/// server list order (like the official picker's sidebar).
pub fn server_sections<'a>(guilds: &'a [Guild], access: &Access<'_>) -> Vec<&'a Guild> {
    let mut out: Vec<&Guild> = guilds
        .iter()
        .filter(|g| {
            g.emojis
                .iter()
                .any(|e| access.emoji(g, e) != Usable::Hidden)
        })
        .collect();
    out.sort_by_key(|g| Some(g.id) != access.here);
    out
}

/// The custom emoji the picker shows: the selected server's, or the
/// matches of every server when searching (open server first). Emoji
/// nobody can use are left out; locked ones stay, with their state.
pub fn shown_custom<'a>(
    picker: &EmojiPicker,
    guilds: &'a [Guild],
    access: &Access<'_>,
) -> Vec<(&'a Guild, &'a CustomEmoji, Usable)> {
    let q = picker.query.trim().trim_matches(':').to_lowercase();
    let sections: Vec<&Guild> = if q.is_empty() {
        match picker.section {
            Section::Server(id) => guilds.iter().filter(|g| g.id == id).collect(),
            Section::Unicode(_) | Section::Recent => vec![],
        }
    } else {
        server_sections(guilds, access)
    };
    let mut out = Vec::new();
    for g in sections {
        for e in &g.emojis {
            if !q.is_empty() && !e.name.to_lowercase().contains(&q) {
                continue;
            }
            let u = access.emoji(g, e);
            if u != Usable::Hidden {
                out.push((g, e, u));
            }
        }
        if !q.is_empty() && out.len() >= MAX_RESULTS {
            out.truncate(MAX_RESULTS);
            break;
        }
    }
    out
}

/// Finds a custom emoji of any server by id, with its server.
pub fn find_custom(guilds: &[Guild], id: Snowflake) -> Option<(&Guild, &CustomEmoji)> {
    guilds
        .iter()
        .find_map(|g| g.emojis.iter().find(|e| e.id == id).map(|e| (g, e)))
}

/// The recently used emoji, most recent first, without duplicates and at
/// most [`RECENT_MAX`]. Kept locally in `meta(RECENT_KEY)`; updated when an
/// emoji is picked, reacted with or sent in a message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recent {
    list: Vec<Emoji>,
}

impl Recent {
    /// Reads the saved list; a missing or broken value is an empty list.
    pub fn load(store: &crate::store::Store) -> Self {
        let list: Vec<Emoji> = store
            .meta(RECENT_KEY)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let mut r = Self::default();
        // Oldest first, so the saved order is kept and the list cleaned.
        for e in list.into_iter().rev() {
            r.record(e);
        }
        r
    }

    /// The JSON saved under [`RECENT_KEY`].
    pub fn to_json(&self) -> Option<String> {
        serde_json::to_string(&self.list).ok()
    }

    pub fn list(&self) -> &[Emoji] {
        &self.list
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Moves `e` to the front (added if new). Unicode emoji are stored in
    /// their fully qualified form, so "❤" and "❤️" are one entry. Returns
    /// whether the list changed.
    pub fn record(&mut self, mut e: Emoji) -> bool {
        if e.id.is_none() {
            match emojis::get(&e.name) {
                Some(u) => e.name = u.as_str().to_owned(),
                // Not an emoji (a hand-edited or broken entry).
                None => return false,
            }
        }
        if self.list.first() == Some(&e) && self.list[0].name == e.name {
            return false;
        }
        self.list.retain(|x| x != &e);
        self.list.insert(0, e);
        self.list.truncate(RECENT_MAX);
        true
    }
}

/// One emoji of the "Recently Used" section, as the picker draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecentItem<'a> {
    Unicode(&'a str),
    Custom(&'a Guild, &'a CustomEmoji, Usable),
}

/// The recent emoji the picker can show, in order. Custom emoji of servers
/// the user left, or that nobody can use any more, are left out; locked ones
/// stay, with their state (like the other sections).
pub fn recent_shown<'a>(
    recent: &'a Recent,
    guilds: &'a [Guild],
    access: &Access<'_>,
) -> Vec<RecentItem<'a>> {
    recent
        .list()
        .iter()
        .filter_map(|e| match e.id {
            None => Some(RecentItem::Unicode(e.name.as_str())),
            Some(id) => {
                let (g, ce) = find_custom(guilds, id)?;
                let u = access.emoji(g, ce);
                (u != Usable::Hidden).then_some(RecentItem::Custom(g, ce, u))
            }
        })
        .collect()
}

/// The first [`QUICK_REACTIONS`] recent emoji the user can react with here.
pub fn quick_reactions<'a>(
    recent: &'a Recent,
    guilds: &'a [Guild],
    access: &Access<'_>,
) -> Vec<RecentItem<'a>> {
    recent_shown(recent, guilds, access)
        .into_iter()
        .filter(|i| !matches!(i, RecentItem::Custom(_, _, u) if *u != Usable::Yes))
        .take(QUICK_REACTIONS)
        .collect()
}

/// The emoji used in a message's text, in order: custom emoji written as
/// `<:name:id>` / `<a:name:id>` and Unicode emoji.
pub fn used_in(text: &str) -> Vec<Emoji> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if rest.starts_with('<')
            && let Some((e, len)) = parse_custom(rest)
        {
            out.push(e);
            rest = &rest[len..];
            continue;
        }
        let Some(g) = rest.graphemes(true).next() else {
            break;
        };
        if crate::ui::emoji_font::is_emoji_cluster(g)
            && let Some(u) = emojis::get(g)
        {
            out.push(Emoji::unicode(u.as_str()));
        }
        rest = &rest[g.len()..];
    }
    out
}

/// A custom emoji tag at the start of `s`, with its length in bytes.
fn parse_custom(s: &str) -> Option<(Emoji, usize)> {
    let end = s.find('>')?;
    let inner = &s[1..end];
    let (animated, inner) = match inner.strip_prefix("a:") {
        Some(r) => (true, r),
        None => (false, inner.strip_prefix(':')?),
    };
    let (name, id) = inner.split_once(':')?;
    let valid_name = (2..=32).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if !valid_name || id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let id: u64 = id.parse().ok()?;
    Some((
        Emoji {
            id: Some(Snowflake(id)),
            name: name.to_owned(),
            animated,
        },
        end + 1,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild_with(emojis: Vec<CustomEmoji>) -> Guild {
        Guild {
            id: Snowflake(1),
            name: "g".into(),
            icon: None,
            roles: vec![],
            owner_id: None,
            emojis,
            stickers: vec![],
            premium_tier: 0,
        }
    }

    #[test]
    fn categories_have_emoji_and_no_skin_variants() {
        for i in 0..CATEGORIES.len() {
            let v = category(i);
            assert!(!v.is_empty(), "category {i}");
            assert!(
                v.iter()
                    .all(|e| e.skin_tone().is_none_or(|t| t == emojis::SkinTone::Default))
            );
        }
        assert!(category(99).is_empty());
    }

    #[test]
    fn search_finds_by_name_and_shortcode() {
        let r = search("thumbs up");
        assert!(r.iter().any(|e| e.as_str() == "👍"));
        let r = search(":crab:");
        assert!(r.iter().any(|e| e.as_str() == "🦀"));
        assert!(search("   ").is_empty());
        assert!(search("zzzzqqqq").is_empty());
        assert!(search("a").len() <= MAX_RESULTS);
    }

    #[test]
    fn prefix_matches_rank_first() {
        let r = search("grin");
        assert!(r[0].name().to_lowercase().starts_with("grin"));
    }

    #[test]
    fn picker_opens_on_server_emoji_when_available() {
        let g = guild_with(vec![CustomEmoji {
            id: Snowflake(5),
            name: "blob".into(),
            animated: false,
            available: true,
            roles: vec![],
        }]);
        let p = EmojiPicker::open(PickerTarget::Composer, Some(&g), false);
        assert_eq!(p.section, Section::Server(Snowflake(1)));
        let p = EmojiPicker::open(PickerTarget::Composer, Some(&guild_with(vec![])), false);
        assert_eq!(p.section, Section::Unicode(0));
        let p = EmojiPicker::open(PickerTarget::Composer, None, false);
        assert_eq!(p.section, Section::Unicode(0));
        // Recently used emoji come first when there are any.
        let p = EmojiPicker::open(PickerTarget::Composer, Some(&g), true);
        assert_eq!(p.section, Section::Recent);
        let p = EmojiPicker::open(PickerTarget::Composer, None, true);
        assert_eq!(p.section, Section::Recent);
    }

    fn access(here: Option<GuildId>, premium_type: u8) -> Access<'static> {
        static ROLES: std::sync::LazyLock<std::collections::HashMap<GuildId, Vec<RoleId>>> =
            std::sync::LazyLock::new(std::collections::HashMap::new);
        Access {
            here,
            premium_type,
            my_roles: &ROLES,
            me: None,
        }
    }

    fn ce(id: u64, name: &str) -> CustomEmoji {
        CustomEmoji {
            id: Snowflake(id),
            name: name.into(),
            animated: false,
            available: true,
            roles: vec![],
        }
    }

    fn two_servers() -> Vec<Guild> {
        let mut a = guild_with(vec![ce(10, "wave"), ce(11, "cat")]);
        a.id = Snowflake(1);
        let mut b = guild_with(vec![ce(20, "wave_big"), {
            let mut e = ce(21, "staff_only");
            e.roles = vec![Snowflake(99)];
            e
        }]);
        b.id = Snowflake(2);
        let mut c = guild_with(vec![{
            let mut e = ce(30, "gone");
            e.available = false;
            e
        }]);
        c.id = Snowflake(3);
        vec![a, b, c]
    }

    #[test]
    fn sidebar_lists_servers_with_visible_emoji_open_one_first() {
        let g = two_servers();
        let ids = |v: Vec<&Guild>| v.iter().map(|g| g.id.0).collect::<Vec<_>>();
        assert_eq!(
            ids(server_sections(&g, &access(Some(Snowflake(2)), 0))),
            vec![2, 1]
        );
        assert_eq!(ids(server_sections(&g, &access(None, 0))), vec![1, 2]);
    }

    #[test]
    fn shown_emoji_carry_their_lock_state() {
        let g = two_servers();
        let acc = access(Some(Snowflake(1)), 0);
        let mut p = EmojiPicker::open(PickerTarget::Composer, g.first(), false);
        let shown = |p: &EmojiPicker, acc: &Access<'_>| {
            shown_custom(p, &g, acc)
                .iter()
                .map(|(g, e, u)| (g.id.0, e.id.0, *u))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            shown(&p, &acc),
            vec![(1, 10, Usable::Yes), (1, 11, Usable::Yes)]
        );
        p.section = Section::Server(Snowflake(2));
        // The role-limited emoji is hidden, the other one locked.
        assert_eq!(shown(&p, &acc), vec![(2, 20, Usable::NeedsNitro)]);
        // Search covers every server, the open one first.
        p.query = ":wave".into();
        assert_eq!(
            shown(&p, &acc),
            vec![(1, 10, Usable::Yes), (2, 20, Usable::NeedsNitro)]
        );
        assert_eq!(
            shown(&p, &access(Some(Snowflake(1)), 2)),
            vec![(1, 10, Usable::Yes), (2, 20, Usable::Yes)]
        );
        p.query.clear();
        p.section = Section::Unicode(0);
        assert!(shown(&p, &acc).is_empty());
        assert_eq!(
            find_custom(&g, Snowflake(20)).map(|(g, e)| (g.id.0, e.name.as_str())),
            Some((2, "wave_big"))
        );
        assert!(find_custom(&g, Snowflake(404)).is_none());
    }

    #[test]
    fn custom_search_skips_unavailable() {
        let g = guild_with(vec![
            CustomEmoji {
                id: Snowflake(5),
                name: "Party_Blob".into(),
                animated: true,
                available: true,
                roles: vec![],
            },
            CustomEmoji {
                id: Snowflake(6),
                name: "party_gone".into(),
                animated: false,
                available: false,
                roles: vec![],
            },
        ]);
        assert_eq!(custom_matching(&g, "party").len(), 1);
        assert_eq!(custom_matching(&g, "").len(), 1);
        assert!(custom_matching(&g, "nope").is_empty());
    }

    fn custom(id: u64, name: &str) -> Emoji {
        Emoji {
            id: Some(Snowflake(id)),
            name: name.into(),
            animated: false,
        }
    }

    #[test]
    fn recent_is_most_recent_first_without_duplicates() {
        let mut r = Recent::default();
        assert!(r.record(Emoji::unicode("👍")));
        assert!(r.record(custom(10, "wave")));
        assert!(r.record(Emoji::unicode("🦀")));
        // Used again: moves to the front instead of being added twice.
        assert!(r.record(Emoji::unicode("👍")));
        assert!(!r.record(Emoji::unicode("👍")));
        // A renamed custom emoji is the same emoji (same id).
        assert!(r.record(custom(10, "wave_renamed")));
        let names: Vec<&str> = r.list().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["wave_renamed", "👍", "🦀"]);
    }

    #[test]
    fn recent_normalises_unicode_and_skips_non_emoji() {
        let mut r = Recent::default();
        assert!(r.record(Emoji::unicode("❤")));
        assert!(!r.record(Emoji::unicode("❤\u{FE0F}")));
        assert_eq!(r.list().len(), 1);
        assert!(!r.record(Emoji::unicode("abc")));
        assert_eq!(r.list().len(), 1);
    }

    #[test]
    fn recent_is_capped() {
        let mut r = Recent::default();
        for e in emojis::iter().take(RECENT_MAX + 10) {
            r.record(Emoji::unicode(e.as_str()));
        }
        assert_eq!(r.list().len(), RECENT_MAX);
        let newest = emojis::iter().nth(RECENT_MAX + 9).map(|e| e.as_str());
        assert_eq!(r.list().first().map(|e| e.name.as_str()), newest);
    }

    #[test]
    fn recent_survives_a_restart() {
        let store = crate::store::Store::open(None).unwrap();
        assert!(Recent::load(&store).is_empty());
        let mut r = Recent::default();
        r.record(Emoji::unicode("🦀"));
        r.record(custom(10, "wave"));
        store.set_meta(RECENT_KEY, &r.to_json().unwrap()).unwrap();
        assert_eq!(Recent::load(&store), r);
        // A broken value is an empty list, not an error.
        store.set_meta(RECENT_KEY, "not json").unwrap();
        assert!(Recent::load(&store).is_empty());
        // Logging out keeps it (`pref.` key).
        store.set_meta(RECENT_KEY, &r.to_json().unwrap()).unwrap();
        store.clear().unwrap();
        assert_eq!(Recent::load(&store), r);
    }

    #[test]
    fn recent_shown_hides_unknown_and_unusable_custom_emoji() {
        let g = two_servers();
        let mut r = Recent::default();
        r.record(custom(404, "left_server"));
        r.record(custom(21, "staff_only"));
        r.record(custom(20, "wave_big"));
        r.record(Emoji::unicode("🦀"));
        r.record(custom(10, "wave"));
        let acc = access(Some(Snowflake(1)), 0);
        let ids = |v: Vec<RecentItem<'_>>| {
            v.iter()
                .map(|i| match i {
                    RecentItem::Unicode(s) => ((*s).to_owned(), Usable::Yes),
                    RecentItem::Custom(_, e, u) => (e.id.0.to_string(), *u),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(recent_shown(&r, &g, &acc)),
            vec![
                ("10".to_owned(), Usable::Yes),
                ("🦀".to_owned(), Usable::Yes),
                ("20".to_owned(), Usable::NeedsNitro),
            ]
        );
        // Quick reactions skip the locked one.
        assert_eq!(
            ids(quick_reactions(&r, &g, &acc)),
            vec![
                ("10".to_owned(), Usable::Yes),
                ("🦀".to_owned(), Usable::Yes)
            ]
        );
    }

    #[test]
    fn used_in_finds_custom_and_unicode_emoji_in_order() {
        let found = used_in("hi 👋 <:wave:10> and <a:party:11>! ❤ <:bad name:12> <:x:abc>");
        let got: Vec<(Option<u64>, &str, bool)> = found
            .iter()
            .map(|e| (e.id.map(|i| i.0), e.name.as_str(), e.animated))
            .collect();
        assert_eq!(
            got,
            vec![
                (None, "👋", false),
                (Some(10), "wave", false),
                (Some(11), "party", true),
                (None, "❤\u{FE0F}", false),
            ]
        );
        assert!(used_in("plain text, 123 #*").is_empty());
    }
}
