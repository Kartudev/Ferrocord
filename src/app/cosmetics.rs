//! App side of profile cosmetics: the popout flow, hover animation, the
//! fetch of cosmetic images and animations, and the lookups the views use.
//!
//! Views never build image handles: they ask the `App` accessors below,
//! which return handles created once when the data arrived (static images
//! in `App::images`, animation frames in `AnimState::cache`).

use std::sync::Arc;
use std::time::Instant;

use iced::Task;
use iced::widget::image::Handle;

use super::anim::{self, Class};
use super::profile::{self, Effects, Popout};
use super::{App, Done, Message, Ui, input, markdown};
use crate::net::cosmetics::{self, AvatarDecoration, Nameplate, PrimaryGuild};
use crate::net::model::*;

/// Avatar sizes requested from the CDN: lists and messages, popout.
pub const AV_LIST: u32 = 80;
pub const AV_POP: u32 = 160;
/// Decoration sizes: lists and messages, popout.
pub const DEC_LIST: u32 = 96;
pub const DEC_POP: u32 = 240;
pub const BANNER_SIZE: u32 = 600;
pub const BADGE_SIZE: u32 = 32;
/// Layers fetched per profile effect.
const MAX_EFFECT_LAYERS: usize = 6;

/// Which clock an animated image follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Static frame: avatars and decorations in lists and messages while
    /// their row is not hovered (the official client's policy).
    Still,
    /// Animates from the start of the current hover: the hovered row's
    /// avatar and decoration, the hovered message's custom emoji.
    Hover,
    /// Animates while the popout is open.
    Popout,
}

impl App {
    fn cdn(&self) -> &str {
        self.cfg.cdn_url.trim_end_matches('/')
    }

    pub fn reduce_motion(&self) -> bool {
        self.prefs.reduce_motion
    }

    /// The phase of an avatar and decoration drawn in the list row or
    /// message `row`. Like the official client (lists, chat, DM list, the
    /// user panel), they are stills and animate only while that row is
    /// hovered; the profile popout animates them all the time
    /// ([`Phase::Popout`]). A still draws no animation widget at all, so
    /// an idle window with cosmetics on screen asks for no redraws.
    pub fn list_phase(&self, row: super::anim::Hover) -> Phase {
        if !self.prefs.reduce_motion && self.anim.hover == Some(row) {
            Phase::Hover
        } else {
            Phase::Still
        }
    }

    /// Whether cosmetics play now (paused, on their first frame, while the
    /// window is in the background).
    pub fn cosmetics_playing(&self) -> bool {
        !self.prefs.reduce_motion && self.window_focused
    }

    /// The clock an animation drawn in `phase` follows: its start (the
    /// widget's epoch) and whether it plays now. Every cosmetic is drawn by
    /// the self-playing `ui::anim_image` widget; none needs an app tick.
    pub fn anim_clock(&self, phase: Phase) -> (Instant, bool) {
        let playing = self.cosmetics_playing();
        match phase {
            Phase::Still => (self.anim.epoch, false),
            Phase::Popout => (self.anim.popout_epoch().unwrap_or(self.anim.epoch), playing),
            Phase::Hover => (self.anim.hover_epoch(), playing),
        }
    }

    /// The decoded animation for a cosmetic `url` drawn in `phase`.
    fn cosmetic_anim(&self, url: &str, phase: Phase) -> Option<&Arc<anim::Anim>> {
        if phase == Phase::Still || self.prefs.reduce_motion {
            return None;
        }
        self.anim.cache.get(url)
    }

    /// The decoded animation of an avatar (`size` is a CDN size:
    /// [`AV_LIST`] or [`AV_POP`]), if cached and `phase` animates.
    pub fn avatar_anim(&self, user: &User, size: u32, phase: Phase) -> Option<&Arc<anim::Anim>> {
        self.cosmetic_anim(&cosmetics::avatar_anim_url(self.cdn(), user, size)?, phase)
    }

    /// The decoded animation of a decoration (`size`: [`DEC_LIST`] or
    /// [`DEC_POP`]), if cached and `phase` animates.
    pub fn decoration_anim(
        &self,
        deco: &AvatarDecoration,
        size: u32,
        phase: Phase,
    ) -> Option<&Arc<anim::Anim>> {
        if !deco.animated() {
            return None;
        }
        self.cosmetic_anim(&deco.url(self.cdn(), size, true), phase)
    }

    /// The static avatar image (`size` is a CDN size).
    pub fn avatar_frame(&self, user: &User, size: u32) -> Option<&Handle> {
        let url = cosmetics::avatar_url(self.cdn(), user, size)?;
        self.images.get(&url).or_else(|| {
            // Fall back to any size we already have.
            self.avatar_handle(user)
        })
    }

    /// The static decoration image (`size`: [`DEC_LIST`] or [`DEC_POP`]).
    pub fn decoration_frame(&self, deco: &AvatarDecoration, size: u32) -> Option<&Handle> {
        self.images.get(&deco.url(self.cdn(), size, false))
    }

    /// The animated banner of the popout (hash `a_…`), if cached.
    pub fn banner_anim(&self, p: &cosmetics::Profile) -> Option<&Arc<anim::Anim>> {
        let hash = p.banner.as_deref().filter(|h| h.starts_with("a_"))?;
        let url = cosmetics::banner_url(
            self.cdn(),
            p.user.id,
            p.banner_guild,
            hash,
            true,
            BANNER_SIZE,
        );
        self.cosmetic_anim(&url, Phase::Popout)
    }

    /// The static banner.
    pub fn banner_frame(&self, p: &cosmetics::Profile) -> Option<&Handle> {
        let hash = p.banner.as_deref()?;
        self.images.get(&cosmetics::banner_url(
            self.cdn(),
            p.user.id,
            p.banner_guild,
            hash,
            false,
            BANNER_SIZE,
        ))
    }

    pub fn nameplate_handle(&self, n: &Nameplate) -> Option<&Handle> {
        self.images.get(&n.static_url(self.cdn()))
    }

    pub fn tag_badge_handle(&self, g: &PrimaryGuild) -> Option<&Handle> {
        self.images.get(&g.badge_url(self.cdn(), BADGE_SIZE)?)
    }

    pub fn badge_icon_handle(&self, icon: &str) -> Option<&Handle> {
        self.images
            .get(&cosmetics::badge_icon_url(self.cdn(), icon))
    }

    pub fn activity_image(&self, a: &Activity, image: &str) -> Option<&Handle> {
        let url = cosmetics::activity_image_url(self.cdn(), a.application_id, image)?;
        self.images.get(&url)
    }

    /// Custom emoji still (the static PNG).
    pub fn emoji_frame(&self, id: Snowflake) -> Option<&Handle> {
        self.image(&super::emoji_url(&self.cdn_url_owned(), id))
    }

    /// With autoplay off, an animated custom emoji plays while its message
    /// is hovered: its animation (when cached) and whether it plays now,
    /// from the start of the hover.
    pub fn emoji_hover_anim(
        &self,
        id: Snowflake,
        animated: bool,
        msg: MessageId,
    ) -> Option<(&Arc<anim::Anim>, Instant, bool)> {
        if !animated || self.prefs.reduce_motion {
            return None;
        }
        let a = self.anim.cache.get(&emoji_anim_url(self.cdn(), id))?;
        let (epoch, playing) = self.anim_clock(Phase::Hover);
        Some((a, epoch, playing && self.anim.is_hovering_message(msg)))
    }

    fn cdn_url_owned(&self) -> String {
        self.cfg.cdn_url.clone()
    }

    /// The popout's profile, once loaded.
    pub fn open_profile(&self) -> Option<(&Popout, Option<&cosmetics::Profile>)> {
        let p = self.profiles.popout.as_ref()?;
        Some((p, self.profiles.get((p.user, p.guild))))
    }

    fn message_emoji_animates(&self, m: MessageId) -> Vec<Snowflake> {
        let Some(c) = self.open_channel() else {
            return vec![];
        };
        let Some(msg) = self.chat.message(c, m) else {
            return vec![];
        };
        if !msg.content.contains("<a:") {
            return vec![];
        }
        let blocks = self.parsed_markdown(m, &msg.content);
        let mut ids = Vec::new();
        collect_animated_emoji(&blocks, &mut ids);
        ids.truncate(24);
        ids
    }

    /// Animations the hovered row could show: the user's animated avatar
    /// and decoration (fetched on the first hover, like the official
    /// client, which asks for the animated asset only when it animates),
    /// and the hovered message's custom emoji.
    fn hover_assets(&self) -> Vec<(String, Class)> {
        let mut v = Vec::new();
        let Some(h) = self.anim.hover else {
            return v;
        };
        if self.prefs.reduce_motion {
            return v;
        }
        let cdn = self.cdn();
        let user = match h.zone {
            super::anim::Zone::Panel => self.chat.me.as_ref(),
            _ => self.chat.users.get(&h.user),
        };
        let user = user.or_else(|| {
            let c = self.open_channel()?;
            let m = self.chat.message(c, h.message?)?;
            Some(&m.author)
        });
        if let Some(u) = user {
            if let Some(url) = cosmetics::avatar_anim_url(cdn, u, AV_LIST) {
                v.push((url, Class::AvatarList));
            }
            if let Some(d) = u.avatar_decoration.as_ref().filter(|d| d.animated()) {
                v.push((d.url(cdn, DEC_LIST, true), Class::DecorationList));
            }
        }
        if let Some(m) = h.message {
            for id in self.message_emoji_animates(m) {
                v.push((emoji_anim_url(cdn, id), Class::Emoji));
            }
        }
        v
    }

    /// Animations of the open popout (avatar, decoration, banner, effect).
    fn popout_assets(&self) -> Vec<(String, Class)> {
        let mut v = Vec::new();
        let Some(po) = self.profiles.popout else {
            return v;
        };
        if self.prefs.reduce_motion {
            return v;
        }
        let cdn = self.cdn();
        let profile = self.profiles.get((po.user, po.guild));
        let user = profile
            .map(|p| p.user.clone())
            .or_else(|| self.chat.users.get(&po.user).cloned());
        if let Some(u) = &user {
            if let Some(url) = cosmetics::avatar_anim_url(cdn, u, AV_POP) {
                v.push((url, Class::Avatar));
            }
            if let Some(d) = u.avatar_decoration.as_ref().filter(|d| d.animated()) {
                v.push((d.url(cdn, DEC_POP, true), Class::Decoration));
            }
        }
        if let Some(p) = profile {
            if let Some(b) = p.banner.as_deref().filter(|b| b.starts_with("a_")) {
                v.push((
                    cosmetics::banner_url(cdn, p.user.id, p.banner_guild, b, true, BANNER_SIZE),
                    Class::Banner,
                ));
            }
            if let Some(e) = p.effect.and_then(|id| self.profiles.effect(id)) {
                for l in e.layers.iter().take(MAX_EFFECT_LAYERS) {
                    let src = l.source_for(p.user.id);
                    if !src.is_empty() && self.rest.is_allowed_media_url(src) {
                        v.push((src.to_owned(), Class::Effect));
                    }
                }
            }
        }
        v
    }

    /// Static cosmetic images wanted now: decorations, nameplates, server
    /// tag badges of visible users, and everything the popout shows.
    pub(super) fn cosmetic_urls(&self) -> Vec<String> {
        let cdn = self.cdn();
        let mut urls = Vec::new();
        if let Some(c) = self.open_channel()
            && let Some(w) = self.chat.messages.get(&c)
        {
            for m in w.messages.iter().rev().take(60) {
                user_urls(cdn, &mut urls, &m.author, false, true);
            }
        }
        if let Some(l) = &self.chat.member_list {
            for item in l.loaded_in(self.member_window()) {
                if let MemberItem::Member(m) = item {
                    user_urls(cdn, &mut urls, &m.user, true, true);
                }
            }
        }
        if let super::View::Home(_) = self.view {
            for c in self.chat.dm_list().into_iter().take(40) {
                if let Some(u) = c.recipients.first().and_then(|u| self.chat.users.get(u)) {
                    user_urls(cdn, &mut urls, u, true, false);
                }
            }
        }
        if let Some(me) = &self.chat.me {
            user_urls(cdn, &mut urls, me, true, false);
        }
        // The open popout / DM profile panel.
        for key in self.profile_keys() {
            let Some(p) = self.profiles.get(key) else {
                continue;
            };
            user_urls(cdn, &mut urls, &p.user, false, true);
            if let Some(d) = &p.user.avatar_decoration {
                urls.push(d.url(cdn, DEC_POP, false));
            }
            if let Some(u) = cosmetics::avatar_url(cdn, &p.user, AV_POP) {
                urls.push(u);
            }
            if let Some(b) = p.banner.as_deref() {
                urls.push(cosmetics::banner_url(
                    cdn,
                    p.user.id,
                    p.banner_guild,
                    b,
                    false,
                    BANNER_SIZE,
                ));
            }
            for b in p.badges.iter().take(12) {
                urls.push(cosmetics::badge_icon_url(cdn, &b.icon));
            }
            for f in p.mutual_friends.iter().take(5) {
                if let Some(u) = cosmetics::avatar_url(cdn, f, 64) {
                    urls.push(u);
                }
            }
            for a in self.chat.activities_of(p.user.id).iter().take(3) {
                for img in [&a.large_image, &a.small_image].into_iter().flatten() {
                    if let Some(u) = cosmetics::activity_image_url(cdn, a.application_id, img) {
                        urls.push(u);
                    }
                }
                if let Some((_, Some(id), _)) = &a.emoji {
                    urls.push(super::emoji_url(cdn, *id));
                }
            }
            if self.prefs.reduce_motion
                && let Some(e) = p.effect.and_then(|id| self.profiles.effect(id))
                && let Some(s) = &e.static_frame
            {
                urls.push(s.clone());
            }
        }
        urls
    }

    /// The profiles currently on screen: the popout and the DM panel.
    pub fn profile_keys(&self) -> Vec<profile::Key> {
        let mut keys = Vec::new();
        if let Some(p) = &self.profiles.popout {
            keys.push((p.user, p.guild));
        }
        if let Some(k) = self.dm_panel_key() {
            keys.push(k);
        }
        keys
    }

    /// Profile key of the 1:1 DM panel when it is open.
    pub fn dm_panel_key(&self) -> Option<profile::Key> {
        if !self.members_open {
            return None;
        }
        let super::View::Home(Some(c)) = self.view else {
            return None;
        };
        let ch = self.chat.channels.get(&c)?;
        let people = self.chat.dm_participants(ch);
        match (ch.kind, people.as_slice()) {
            (ChannelKind::Dm, [other]) => Some((other.id, None)),
            _ => None,
        }
    }

    /// Fetch the animations wanted now (hover + popout).
    pub(super) fn request_anims(&mut self) -> Task<Message> {
        if self.cfg.mock || self.prefs.reduce_motion {
            return Task::none();
        }
        let now = Instant::now();
        // Message GIFs and stickers are decoded from the image loader's
        // bytes (`decode_media_anim`); they are fetched here only when that
        // already happened and the animation was evicted since.
        let messages = self.message_anim_assets().into_iter().filter(|(k, c)| {
            matches!(c, Class::Emoji | Class::Gifv) || self.images.get(k).is_some()
        });
        let wanted: Vec<(String, Class)> = self
            .hover_assets()
            .into_iter()
            .chain(self.popout_assets())
            .chain(messages)
            .collect();
        let mut tasks = Vec::new();
        for (url, class) in wanted {
            if !self.rest.is_allowed_media_url(&url) {
                continue;
            }
            if self.anim.cache.contains(&url) {
                self.anim.cache.touch(&url);
                continue;
            }
            if !self.anim.cache.want(&url, now) {
                continue;
            }
            if class == Class::Gifv {
                tasks.push(self.gifv_task(url));
                continue;
            }
            let rest = self.rest.clone();
            tasks.push(Task::perform(
                async move {
                    let anim = match rest.fetch_bytes(&url).await {
                        Ok(b) => tokio::task::spawn_blocking(move || {
                            anim::decode(&b, &class.limits()).ok().map(Arc::new)
                        })
                        .await
                        .ok()
                        .flatten(),
                        Err(_) => None,
                    };
                    (url, anim)
                },
                move |(key, anim)| Message::Done(Done::Anim { key, class, anim }),
            ));
        }
        Task::batch(tasks)
    }

    /// Requests the profile of everything on screen that needs one.
    pub(super) fn request_profiles(&mut self) -> Task<Message> {
        let now = Instant::now();
        let mut tasks = Vec::new();
        for key in self.profile_keys() {
            if !self.profiles.want(key, now) {
                continue;
            }
            if self.cfg.mock {
                let p = crate::mock::profile(key.0, key.1);
                self.profiles.set(key, p, now);
                continue;
            }
            let rest = self.rest.clone();
            tasks.push(Task::perform(
                async move {
                    rest.user_profile(key.0, key.1)
                        .await
                        .map_err(|e| e.user_message())
                },
                move |result| {
                    Message::Done(Done::Profile {
                        key,
                        result: result.map(Box::new),
                    })
                },
            ));
        }
        // Effect configs, once a shown profile has an effect.
        let needs_effects = self
            .profile_keys()
            .into_iter()
            .filter_map(|k| self.profiles.get(k))
            .any(|p| p.effect.is_some());
        if needs_effects && self.profiles.want_effects(now) {
            if self.cfg.mock {
                self.profiles
                    .set_effects(Some(crate::mock::profile_effects()), now);
            } else {
                let rest = self.rest.clone();
                tasks.push(Task::perform(
                    async move { rest.profile_effects().await.map_err(|e| e.user_message()) },
                    |r| Message::Done(Done::ProfileEffects(r)),
                ));
            }
        }
        if self.cfg.mock {
            self.mock_fill_popout();
        }
        Task::batch(tasks)
    }

    pub(super) fn update_cosmetics(&mut self, u: Ui) -> Task<Message> {
        match u {
            Ui::OpenProfile { user, guild } => self.open_profile_popout(user, guild),
            Ui::CloseProfile => {
                self.profiles.popout = None;
                self.anim.close_popout();
                Task::none()
            }
            Ui::HoverEnter(h) => {
                self.anim.enter(h, Instant::now());
                if !self.prefs.reduce_motion && !self.hover_assets().is_empty() {
                    self.request_anims()
                } else {
                    Task::none()
                }
            }
            Ui::HoverLeave(h) => {
                self.anim.leave(h);
                Task::none()
            }
            Ui::ToggleReduceMotion => {
                self.prefs.reduce_motion = !self.prefs.reduce_motion;
                let t = self.save_pref(super::window::KEY_REDUCE_MOTION, self.prefs.reduce_motion);
                Task::batch([t, self.request_images()])
            }
            Ui::ToggleAutoplayGifs => {
                // Synced as `gif_auto_play`; the local pref mirrors it so the
                // last value is known before Discord answers.
                use crate::net::user_settings::{Edit, Synced, Value};
                let on = !self.settings.synced.bool(Synced::GifAutoPlay);
                let t = self.edit_synced(Edit::Set(Synced::GifAutoPlay, Value::Bool(on)));
                let save = self.save_pref(super::window::KEY_AUTOPLAY_GIFS, on);
                Task::batch([t, save, self.request_images()])
            }
            _ => Task::none(),
        }
    }

    fn open_profile_popout(&mut self, user: UserId, guild: Option<GuildId>) -> Task<Message> {
        self.menu = None;
        self.ctx_menu = None;
        self.emoji_picker = None;
        self.status_menu = false;
        let window = input::window_size();
        let c = input::cursor();
        // Beside the pointer, on the side with more room (the member list
        // is on the right edge).
        let x = if c.x > window.width / 2.0 {
            c.x - profile::CARD_W - 16.0
        } else {
            c.x + 16.0
        };
        let at = input::clamp_popup(
            iced::Point::new(x, c.y - 24.0),
            iced::Size::new(profile::CARD_W, profile::CARD_H),
            window,
        );
        self.profiles.popout = Some(Popout { user, guild, at });
        self.anim.open_popout(Instant::now());
        let t = self.request_profiles();
        Task::batch([t, self.request_images(), self.request_anims()])
    }

    pub(super) fn on_cosmetics_done(&mut self, d: Done) -> Task<Message> {
        let now = Instant::now();
        match d {
            Done::Profile { key, result } => {
                self.profiles.set(key, result.ok().map(|p| *p), now);
            }
            Done::ProfileEffects(r) => self.profiles.set_effects(r.ok(), now),
            Done::Anim { key, class, anim } => match anim {
                Some(a) => self.anim.cache.insert(key, a, class),
                None => self.anim.cache.mark_failed(&key, now),
            },
            _ => {}
        }
        let t = self.request_profiles();
        Task::batch([t, self.request_images(), self.request_anims()])
    }

    /// Effect layer frames for the popout card: `(layer, frame)` bottom to
    /// top. Layers outside their timeline are skipped.
    pub fn effect_layers(&self) -> Vec<EffectDraw<'_>> {
        let Some((_, Some(p))) = self.open_profile() else {
            return vec![];
        };
        let Some(e) = p.effect.and_then(|id| self.profiles.effect(id)) else {
            return vec![];
        };
        if self.prefs.reduce_motion {
            return e
                .static_frame
                .as_deref()
                .and_then(|u| self.images.get(u))
                .map(|h| {
                    vec![EffectDraw {
                        image: EffectImage::Still(h),
                        x: 0,
                        y: 0,
                        w: 0,
                        h: 0,
                    }]
                })
                .unwrap_or_default();
        }
        let ms = |v: u32| std::time::Duration::from_millis(u64::from(v));
        e.layers
            .iter()
            .take(MAX_EFFECT_LAYERS)
            .filter_map(|l| {
                let a = self.anim.cache.get(l.source_for(p.user.id))?;
                let timeline = crate::ui::anim_image::Timeline {
                    start: ms(l.start_ms),
                    duration: ms(l.duration_ms),
                    looped: l.looped,
                    loop_delay: ms(l.loop_delay_ms),
                };
                Some(EffectDraw {
                    image: EffectImage::Anim(a, timeline),
                    x: l.x,
                    y: l.y,
                    w: l.width,
                    h: l.height,
                })
            })
            .collect()
    }

    /// Canvas size the effect layers are placed in (for scaling to the
    /// card): the union of every layer's rectangle.
    pub fn effect_canvas(&self) -> (f32, f32) {
        let Some((_, Some(p))) = self.open_profile() else {
            return (1.0, 1.0);
        };
        let Some(e) = p.effect.and_then(|id| self.profiles.effect(id)) else {
            return (1.0, 1.0);
        };
        e.layers.iter().fold((1.0f32, 1.0f32), |(w, h), l| {
            (
                w.max((l.x.max(0) as u32 + l.width) as f32),
                h.max((l.y.max(0) as u32 + l.height) as f32),
            )
        })
    }

    /// Mock mode: inserts the popout-only animations (banner, effect).
    fn mock_fill_popout(&mut self) {
        if self.profiles.popout.is_none() {
            return;
        }
        for (key, class, a) in crate::mock::cosmetics::popout_anims(&self.cfg.cdn_url) {
            if !self.anim.cache.contains(&key) {
                self.anim.cache.insert(key, Arc::new(a), class);
            }
        }
    }

    /// Mock mode: static images and animations of the mock cosmetics.
    pub(super) fn mock_load_cosmetics(&mut self) {
        let cdn = self.cfg.cdn_url.clone();
        for (url, w, h, px) in crate::mock::cosmetics::images(&cdn) {
            self.images
                .insert(url, Handle::from_rgba(w, h, px), (w * h * 4) as usize);
        }
        for (key, class, a) in crate::mock::cosmetics::anims(&cdn) {
            self.anim.cache.insert(key, Arc::new(a), class);
        }
    }

    /// Effects state, for the loading hint and tests.
    pub fn effects_ready(&self) -> bool {
        matches!(self.profiles.effects, Effects::Ready(_))
    }
}

/// One effect layer to draw.
#[derive(Debug)]
pub struct EffectDraw<'a> {
    pub image: EffectImage<'a>,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// What an effect layer shows.
#[derive(Debug)]
pub enum EffectImage<'a> {
    /// The static frame (reduced motion).
    Still(&'a Handle),
    /// An animated layer, shown during its timeline (counted from the
    /// popout opening).
    Anim(&'a Arc<anim::Anim>, crate::ui::anim_image::Timeline),
}

/// Static cosmetic images of one user (decoration, nameplate, tag badge).
fn user_urls(cdn: &str, urls: &mut Vec<String>, u: &User, plate: bool, tag: bool) {
    if let Some(d) = &u.avatar_decoration {
        urls.push(d.url(cdn, DEC_LIST, false));
    }
    if plate && let Some(n) = u.collectibles.as_ref().and_then(|c| c.nameplate.as_ref()) {
        urls.push(n.static_url(cdn));
    }
    if tag
        && let Some(g) = u
            .primary_guild
            .as_ref()
            .filter(|g| g.visible_tag().is_some())
        && let Some(b) = g.badge_url(cdn, BADGE_SIZE)
    {
        urls.push(b);
    }
}

pub fn emoji_anim_url(cdn: &str, id: Snowflake) -> String {
    format!("{}/emojis/{id}.gif?size=96", cdn.trim_end_matches('/'))
}

fn collect_animated_emoji(blocks: &[markdown::Block], out: &mut Vec<Snowflake>) {
    fn walk(v: &[markdown::Inline], out: &mut Vec<Snowflake>) {
        use markdown::Inline as I;
        for i in v {
            match i {
                I::Emoji {
                    id, animated: true, ..
                } => out.push(*id),
                I::Bold(c) | I::Italic(c) | I::Underline(c) | I::Strike(c) | I::Spoiler(c) => {
                    walk(c, out)
                }
                I::Link { label, .. } => walk(label, out),
                _ => {}
            }
        }
    }
    for b in blocks {
        match b {
            markdown::Block::Paragraph(v)
            | markdown::Block::Heading(_, v)
            | markdown::Block::Subtext(v) => walk(v, out),
            markdown::Block::Quote(inner) => collect_animated_emoji(inner, out),
            markdown::Block::List(items) => items.iter().for_each(|v| walk(v, out)),
            markdown::Block::CodeBlock { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_gif_url() {
        assert_eq!(
            emoji_anim_url("https://cdn.discordapp.com/", Snowflake(5)),
            "https://cdn.discordapp.com/emojis/5.gif?size=96"
        );
    }

    #[test]
    fn animated_emoji_are_collected() {
        let blocks = markdown::parse("hi <a:wave:12> and <:still:13> **<a:x:14>**");
        let mut ids = vec![];
        collect_animated_emoji(&blocks, &mut ids);
        assert_eq!(ids, vec![Snowflake(12), Snowflake(14)]);
    }
}
