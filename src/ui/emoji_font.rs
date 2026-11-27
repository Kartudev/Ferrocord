//! Colour emoji, drawn with Twemoji as the official client does.
//!
//! Unicode emoji are drawn with the bundled Twemoji font
//! (`theme::TWEMOJI`: jdecked/twemoji graphics, CC-BY 4.0, compiled to a
//! COLRv0 font by Mozilla's twemoji-colr pipeline) in their own text spans.
//! Text is split by grapheme cluster so keycaps, flags, skin tones and ZWJ
//! sequences are never cut apart; the font's `ccmp` ligatures then turn each
//! cluster into one colour glyph.
//!
//! Why COLRv0: swash (cosmic-text's rasteriser) draws COLRv0 layers in
//! colour (`Source::ColorOutline`) and bitmap glyphs (CBDT/sbix), but not
//! COLRv1. COLRv0 is vector, so emoji stay sharp at every size, and the
//! whole font is ~2 MB (Noto Color Emoji's CBDT build is 10.8 MB).
//!
//! The font has its own family name, "Ferrocord Twemoji", so no system face
//! can shadow it (font lookup returns the first face of a family, and system
//! fonts load first). [`install`] loads it, and the UI font (Inter), zero-copy
//! into iced's font system at startup and puts it in cosmic-text's common
//! fallback list right after Inter: text that cannot take spans (the
//! composer's `text_editor`, text inputs) gets Twemoji for every emoji the
//! text font lacks, not the system emoji font.
//!
//! Twemoji must never be the FIRST fallback. Basic shaping (iced's `Auto`
//! for ASCII text) takes the first fallback font as-is when the requested
//! face is missing, without checking coverage, and Twemoji maps no letters:
//! every glyph became `.notdef`, a box. Its PostScript name contains "Emoji",
//! which cosmic-text matches for every style, so it also outranked a missing
//! italic. Inter (every weight and style the views use is bundled) comes
//! first, so a missing face resolves to real text.

use std::sync::Arc;

use iced::advanced::graphics::text::cosmic_text;
use iced::widget::text::{Span, Wrapping};
use iced::widget::{rich_text, span, text};
use iced::{Color, Element, Font};

use crate::app::Message;

/// Family name of the bundled Twemoji font.
pub const FAMILY: &str = "Ferrocord Twemoji";

/// The colour emoji font for spans.
pub fn emoji_font() -> Font {
    Font::with_name(FAMILY)
}

/// cosmic-text fallback: the platform lists, with Inter and then Twemoji in
/// front of the common fallbacks, so Twemoji wins over the system emoji font
/// but never stands in for a missing text face.
struct UiFallback {
    inner: cosmic_text::PlatformFallback,
    common: Vec<&'static str>,
}

impl UiFallback {
    fn new() -> Self {
        use cosmic_text::Fallback;
        let inner = cosmic_text::PlatformFallback;
        let mut common = vec![crate::ui::theme::UI_FAMILY, FAMILY];
        common.extend(inner.common_fallback().iter().copied());
        Self { inner, common }
    }
}

impl cosmic_text::Fallback for UiFallback {
    fn common_fallback(&self) -> &[&'static str] {
        &self.common
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        self.inner.forbidden_fallback()
    }

    fn script_fallback(&self, script: unicode_script::Script, locale: &str) -> &[&'static str] {
        self.inner.script_fallback(script, locale)
    }
}

/// Loads the bundled UI fonts and Twemoji into `fs` without copying them
/// and sets the fallback order (Inter, Twemoji, platform lists).
///
/// Inter is loaded before Twemoji: when no face matches a weight exactly,
/// cosmic-text breaks ties between equally close faces by load order.
pub fn install_into(fs: &mut cosmic_text::FontSystem) {
    let empty = cosmic_text::FontSystem::new_with_locale_and_db(
        String::new(),
        cosmic_text::fontdb::Database::new(),
    );
    let (locale, mut db) = std::mem::replace(fs, empty).into_locale_and_db();
    for bytes in crate::ui::theme::FONT_FILES {
        db.load_font_source(cosmic_text::fontdb::Source::Binary(Arc::new(bytes)));
    }
    db.load_font_source(cosmic_text::fontdb::Source::Binary(Arc::new(
        crate::ui::theme::TWEMOJI,
    )));
    *fs =
        cosmic_text::FontSystem::new_with_locale_and_db_and_fallback(locale, db, UiFallback::new());
}

/// Installs Inter and Twemoji in iced's global font system. Call once in
/// `main`, before `run`: this works without a window, unlike
/// `iced::font::load`, which a daemon drops while no window (and so no
/// renderer) exists, e.g. when started with `--minimized`.
pub fn install() {
    let mut fs = iced::advanced::graphics::text::font_system()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    install_into(fs.raw());
}

const VS16: char = '\u{FE0F}';
const KEYCAP: char = '\u{20E3}';

/// Single code points that are drawn as emoji even without a variation
/// selector: the pictograph planes, the BMP symbol blocks and the stray
/// emoji-presentation characters outside them (Unicode `Emoji_Presentation`
/// plus the classic Telegram set). Text-default marks such as (c), (R) and
/// TM only become emoji with U+FE0F, which `is_emoji_cluster` checks.
pub fn is_emoji(c: char) -> bool {
    matches!(c as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0x2300..=0x23FF
        | 0x25FB..=0x25FE | 0x203C | 0x2049 | 0x3030 | 0x303D | 0x3297 | 0x3299
        | 0xFE0F | 0x20E3)
}

/// Whether a grapheme cluster is an emoji sequence: a keycap, a flag, a
/// variation-selector-16 presentation sequence, a skin-toned or ZWJ sequence,
/// or a pictograph. Plain digits, `#`, `*` and bare (c)/(R)/TM stay text.
pub fn is_emoji_cluster(g: &str) -> bool {
    g.chars().any(|c| {
        c == VS16
            || c == KEYCAP
            || (c != '\u{200D}' && is_emoji(c))
            || matches!(c as u32, 0x1F3FB..=0x1F3FF)
    })
}

/// Split into (is_emoji, run) segments, by grapheme cluster so that a keycap,
/// flag or ZWJ sequence is never cut apart.
pub fn emoji_runs(s: &str) -> Vec<(bool, String)> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut out: Vec<(bool, String)> = Vec::new();
    for g in s.graphemes(true) {
        let e = is_emoji_cluster(g);
        match out.last_mut() {
            Some((k, run)) if *k == e => run.push_str(g),
            _ => out.push((e, g.to_string())),
        }
    }
    out
}

/// True when the text has at least one emoji cluster.
pub fn has_emoji(s: &str) -> bool {
    use unicode_segmentation::UnicodeSegmentation;
    s.graphemes(true).any(is_emoji_cluster)
}

/// Text that renders its emoji with the colour emoji font. `font` is the
/// face of the surrounding text; `wrap` allows word wrapping.
pub fn emoji_text<'a>(
    s: impl Into<String>,
    size: f32,
    color: Color,
    font: Font,
    wrap: bool,
) -> Element<'a, Message> {
    let s = s.into();
    let wrapping = if wrap { Wrapping::Word } else { Wrapping::None };
    if !has_emoji(&s) {
        return text(s)
            .size(size)
            .color(color)
            .font(font)
            .wrapping(wrapping)
            .into();
    }
    let spans: Vec<Span<'a, (), Font>> = emoji_runs(&s)
        .into_iter()
        .map(|(e, run)| {
            let sp = span(run).color(color);
            if e { sp.font(emoji_font()) } else { sp }
        })
        .collect();
    rich_text::<(), Message, _, _>(spans)
        .size(size)
        .font(font)
        .wrapping(wrapping)
        .into()
}

/// Splits `s` into spans for a rich-text paragraph: the text spans share
/// `base`, emoji runs use the colour emoji family. `style` customises each.
pub fn emoji_spans<'a, L: Clone>(
    s: &str,
    mut style: impl FnMut(Span<'a, L, Font>) -> Span<'a, L, Font>,
) -> Vec<Span<'a, L, Font>> {
    emoji_runs(s)
        .into_iter()
        .map(|(e, run)| {
            let sp = style(span(run));
            if e { sp.font(emoji_font()) } else { sp }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_use_the_twemoji_family() {
        assert_eq!(emoji_font(), Font::with_name(FAMILY));
    }

    use cosmic_text::{
        Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
    };

    /// A font system with only the bundled fonts (no system font at all),
    /// built the way `install` builds it.
    fn twemoji_only() -> FontSystem {
        let mut fs = FontSystem::new_with_locale_and_db(
            "en-US".into(),
            cosmic_text::fontdb::Database::new(),
        );
        install_into(&mut fs);
        fs
    }

    /// Shapes `s` with Twemoji and rasterises every glyph: returns, per
    /// glyph, whether swash drew it in colour.
    fn render(fs: &mut FontSystem, s: &str, family: Family<'_>) -> Vec<bool> {
        let mut buf = Buffer::new(fs, Metrics::new(32.0, 40.0));
        buf.set_text(fs, s, &Attrs::new().family(family), Shaping::Advanced, None);
        buf.shape_until_scroll(fs, false);
        let glyphs: Vec<_> = buf
            .layout_runs()
            .flat_map(|r| r.glyphs.iter().cloned())
            .collect();
        let mut cache = SwashCache::new();
        glyphs
            .iter()
            .map(|g| {
                let pg = g.physical((0.0, 0.0), 1.0);
                cache
                    .get_image_uncached(fs, pg.cache_key)
                    .is_some_and(|img| img.content == SwashContent::Color && !img.data.is_empty())
            })
            .collect()
    }

    #[test]
    fn bundled_font_is_colrv0_under_its_own_family() {
        let face = ttf_parser::Face::parse(crate::ui::theme::TWEMOJI, 0).unwrap();
        let families: Vec<String> = face
            .names()
            .into_iter()
            .filter(|n| n.name_id == ttf_parser::name_id::FAMILY)
            .filter_map(|n| n.to_string())
            .collect();
        assert!(!families.is_empty());
        assert!(families.iter().all(|f| f == FAMILY), "{families:?}");
        let t = face.tables();
        assert!(t.colr.is_some());
        // Kept small: the whole point over the 10.8 MB CBDT Noto.
        assert!(crate::ui::theme::TWEMOJI.len() < 3 * 1024 * 1024);
    }

    /// Every kind of cluster shapes to ONE glyph that swash draws in
    /// colour: the segmentation and the font's ligatures agree.
    #[test]
    fn sequences_render_as_one_colour_glyph() {
        let mut fs = twemoji_only();
        let cases = [
            ("grinning", "\u{1F600}"),
            ("heart vs16", "\u{2764}\u{FE0F}"),
            ("heart bare", "\u{2764}"),
            ("keycap vs16", "1\u{FE0F}\u{20E3}"),
            ("keycap bare", "#\u{20E3}"),
            ("flag", "\u{1F1EA}\u{1F1F8}"),
            (
                "england",
                "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}",
            ),
            ("skin tone", "\u{1F44B}\u{1F3FF}"),
            (
                "family zwj",
                "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}",
            ),
            ("rainbow flag", "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}"),
            (
                "health worker, tone",
                "\u{1F9D1}\u{1F3FD}\u{200D}\u{2695}\u{FE0F}",
            ),
            (
                "handshake, two tones",
                "\u{1FAF1}\u{1F3FB}\u{200D}\u{1FAF2}\u{1F3FF}",
            ),
            ("Unicode 15.1 phoenix", "\u{1F426}\u{200D}\u{1F525}"),
            ("(c) vs16", "\u{A9}\u{FE0F}"),
        ];
        for (name, s) in cases {
            // Segmentation keeps the whole cluster as one emoji run...
            assert_eq!(emoji_runs(s), vec![(true, s.to_string())], "{name}");
            // ...and the font turns it into one colour glyph.
            let g = render(&mut fs, s, Family::Name(FAMILY));
            assert_eq!(g, vec![true], "{name}: {g:?}");
        }
        // Two clusters in one run stay two glyphs.
        let g = render(
            &mut fs,
            "\u{1F1EA}\u{1F1F8}\u{1F1EB}\u{1F1F7}",
            Family::Name(FAMILY),
        );
        assert_eq!(g, vec![true, true]);
    }

    /// Text that cannot take spans (the composer) falls back to Twemoji,
    /// even with the system fonts loaded (a system Noto Color Emoji, the
    /// platform's first emoji fallback, would otherwise win).
    #[test]
    fn fallback_prefers_twemoji_over_system_fonts() {
        use cosmic_text::Fallback;
        assert_eq!(
            UiFallback::new().common_fallback()[..2],
            [crate::ui::theme::UI_FAMILY, FAMILY]
        );
        let mut fs = FontSystem::new();
        install_into(&mut fs);
        let mut buf = Buffer::new(&mut fs, Metrics::new(32.0, 40.0));
        buf.set_text(
            &mut fs,
            "hi \u{1F600} \u{1F44B}\u{1F3FD}",
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );
        buf.shape_until_scroll(&mut fs, false);
        let fonts: Vec<_> = buf
            .layout_runs()
            .flat_map(|r| r.glyphs.iter())
            .filter(|g| g.start >= 3 && !buf.lines[0].text()[g.start..g.end].trim().is_empty())
            .map(|g| g.font_id)
            .collect();
        assert_eq!(fonts.len(), 2, "{fonts:?}");
        for id in fonts {
            let face = fs.db().face(id).unwrap();
            assert!(
                face.families.iter().any(|(f, _)| f == FAMILY),
                "{:?}",
                face.families
            );
        }
    }

    /// (family, glyph id) of every glyph of `s` shaped with `attrs`.
    fn shaped(
        fs: &mut FontSystem,
        s: &str,
        attrs: &Attrs<'_>,
        shaping: Shaping,
    ) -> Vec<(String, u16)> {
        let mut buf = Buffer::new(fs, Metrics::new(16.0, 20.0));
        buf.set_text(fs, s, attrs, shaping, None);
        buf.shape_until_scroll(fs, false);
        let glyphs: Vec<_> = buf
            .layout_runs()
            .flat_map(|r| r.glyphs.iter().map(|g| (g.font_id, g.glyph_id)))
            .collect();
        glyphs
            .into_iter()
            .map(|(id, glyph)| {
                let family = fs.db().face(id).map(|f| f.families[0].0.clone());
                (family.unwrap_or_default(), glyph)
            })
            .collect()
    }

    /// Regression (v0.19 "texts got broken"): the UI fonts were loaded with
    /// `iced::font::load`, which the daemon drops while no window exists
    /// (`--minimized`, autostart), so Inter was missing and ASCII text
    /// (Basic shaping) took the first common fallback, Twemoji: boxes.
    /// `install_into` alone must provide every face the views use.
    #[test]
    fn install_provides_every_ui_face() {
        use crate::ui::theme::{INTER, INTER_BOLD, INTER_ITALIC, INTER_MEDIUM, INTER_SEMIBOLD};
        let fs = twemoji_only();
        for font in [
            INTER,
            INTER_MEDIUM,
            INTER_SEMIBOLD,
            INTER_BOLD,
            INTER_ITALIC,
        ] {
            let attrs = iced::advanced::graphics::text::to_attributes(font);
            let face = fs.db().faces().find(|f| {
                f.families
                    .iter()
                    .any(|(n, _)| n == crate::ui::theme::UI_FAMILY)
                    && f.weight == attrs.weight
                    && f.style == attrs.style
            });
            assert!(face.is_some(), "no bundled face for {font:?}");
        }
    }

    /// Latin text never lands on Twemoji or `.notdef`, whatever face is
    /// asked for (every weight, both styles, a family that does not exist)
    /// and whichever shaping iced picks, with no system font installed.
    #[test]
    fn text_never_falls_back_to_twemoji() {
        let mut fs = twemoji_only();
        let families = [
            Family::Name(crate::ui::theme::UI_FAMILY),
            Family::Name("No Such Family"),
            Family::SansSerif,
            Family::Monospace,
        ];
        for family in families {
            for weight in (100..=900).step_by(100) {
                for style in [cosmic_text::Style::Normal, cosmic_text::Style::Italic] {
                    for shaping in [Shaping::Basic, Shaping::Advanced] {
                        let attrs = Attrs::new()
                            .family(family)
                            .weight(cosmic_text::Weight(weight))
                            .style(style);
                        let s = "Friends 0123 #general Base A\u{E9}rea";
                        let glyphs = shaped(&mut fs, s, &attrs, shaping);
                        assert!(!glyphs.is_empty());
                        for (f, g) in glyphs {
                            let what =
                                format!("{family:?} {weight} {style:?} {shaping:?}: {f} {g}");
                            assert_ne!(f, FAMILY, "{what}");
                            assert_ne!(g, 0, "{what}");
                        }
                    }
                }
            }
        }
    }

    /// The composer's case: Inter text with emoji, no spans. Letters stay
    /// Inter, emoji come from Twemoji.
    #[test]
    fn emoji_in_inter_text_use_twemoji() {
        let mut fs = twemoji_only();
        let attrs = Attrs::new().family(Family::Name(crate::ui::theme::UI_FAMILY));
        let glyphs = shaped(&mut fs, "hi\u{1F980}ok\u{2615}", &attrs, Shaping::Advanced);
        let families: Vec<&str> = glyphs.iter().map(|(f, _)| f.as_str()).collect();
        let ui = crate::ui::theme::UI_FAMILY;
        assert_eq!(families, [ui, ui, FAMILY, ui, ui, FAMILY], "{glyphs:?}");
        assert!(glyphs.iter().all(|(_, g)| *g != 0), "{glyphs:?}");
    }

    fn kinds(s: &str) -> Vec<(bool, String)> {
        emoji_runs(s)
    }

    #[test]
    fn emoji_runs_split() {
        let r = emoji_runs("hi \u{1F44D}\u{1F3FD} ok \u{2764}\u{FE0F}");
        assert_eq!(r.len(), 4);
        assert!(r[1].0 && !r[2].0 && r[3].0);
        assert_eq!(r[3].1, "\u{2764}\u{FE0F}");
        assert!(!is_emoji('a'));
    }

    #[test]
    fn keycaps_stay_whole() {
        for k in [
            "1\u{FE0F}\u{20E3}",
            "#\u{FE0F}\u{20E3}",
            "*\u{FE0F}\u{20E3}",
            "0\u{20E3}",
        ] {
            assert_eq!(kinds(k), vec![(true, k.to_string())], "{k:?}");
        }
        assert_eq!(kinds("a1#"), vec![(false, "a1#".to_string())]);
        assert_eq!(
            kinds("call 112 #1"),
            vec![(false, "call 112 #1".to_string())]
        );
        assert_eq!(
            kinds("x 1\u{FE0F}\u{20E3} y"),
            vec![
                (false, "x ".into()),
                (true, "1\u{FE0F}\u{20E3}".into()),
                (false, " y".into())
            ]
        );
    }

    #[test]
    fn flags() {
        let es = "\u{1F1EA}\u{1F1F8}";
        let fr = "\u{1F1EB}\u{1F1F7}";
        assert_eq!(
            kinds(&format!("{es}{fr}")),
            vec![(true, format!("{es}{fr}"))]
        );
        // England: black flag + tag sequence.
        let eng = "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}";
        assert_eq!(kinds(eng), vec![(true, eng.to_string())]);
    }

    #[test]
    fn zwj_families_and_professions() {
        let fam = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
        assert_eq!(kinds(fam), vec![(true, fam.to_string())]);
        let heart = "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}";
        assert_eq!(kinds(heart), vec![(true, heart.to_string())]);
        // A ZWJ sequence whose tail is outside the pictograph blocks.
        let med = "\u{1F9D1}\u{1F3FD}\u{200D}\u{2695}\u{FE0F}";
        assert_eq!(kinds(med), vec![(true, med.to_string())]);
        let walk = "\u{1F6B6}\u{200D}\u{2640}\u{FE0F}";
        assert_eq!(kinds(walk), vec![(true, walk.to_string())]);
    }

    #[test]
    fn skin_tones() {
        let t = "\u{1F44B}\u{1F3FF}";
        assert_eq!(kinds(t), vec![(true, t.to_string())]);
        assert_eq!(kinds("hi\u{1F44B}\u{1F3FB}!").len(), 3);
    }

    #[test]
    fn symbols_with_and_without_vs16() {
        for c in ['\u{A9}', '\u{AE}', '\u{2122}', '\u{2139}', '\u{2194}'] {
            let bare = c.to_string();
            let vs = format!("{c}\u{FE0F}");
            assert_eq!(kinds(&bare), vec![(false, bare.clone())], "bare {c:?}");
            assert_eq!(kinds(&vs), vec![(true, vs.clone())], "vs16 {c:?}");
        }
        for c in [
            '\u{203C}', '\u{2049}', '\u{3030}', '\u{303D}', '\u{3297}', '\u{3299}',
        ] {
            assert!(has_emoji(&c.to_string()), "{c:?}");
            assert!(has_emoji(&format!("{c}\u{FE0F}")), "{c:?}");
        }
    }

    #[test]
    fn mixed_text() {
        let r = kinds(
            "caf\u{E9} \u{2615} \u{1F600}\u{1F600} \u{43F}\u{440}\u{438}\u{432}\u{435}\u{442} \u{1F1EA}\u{1F1F8}",
        );
        let emoji: Vec<&str> = r
            .iter()
            .filter(|(e, _)| *e)
            .map(|(_, s)| s.as_str())
            .collect();
        assert_eq!(
            emoji,
            vec!["\u{2615}", "\u{1F600}\u{1F600}", "\u{1F1EA}\u{1F1F8}"]
        );
        let text: String = r
            .iter()
            .filter(|(e, _)| !*e)
            .map(|(_, s)| s.as_str())
            .collect();
        assert_eq!(
            text,
            "caf\u{E9}   \u{43F}\u{440}\u{438}\u{432}\u{435}\u{442} "
        );
        assert!(kinds("").is_empty());
        assert!(!has_emoji("plain 123 #"));
    }
}
