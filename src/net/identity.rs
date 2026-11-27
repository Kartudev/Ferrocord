//! What Ferrocord tells Discord about itself.
//!
//! Ferrocord presents itself as a Chrome browser session so that its TLS
//! fingerprint, `X-Super-Properties` and gateway IDENTIFY all look like a
//! real browser tab on `discord.com`:
//!
//! * `X-Super-Properties`: base64 of the same JSON object the gateway
//!   IDENTIFY carries in `properties` (one source, so both always agree);
//! * `X-Discord-Locale`: the system UI language (`en-US`);
//! * `X-Discord-Timezone`: the IANA time zone (`Europe/Madrid`);
//! * `User-Agent`: Chrome's UA string.
//!
//! The locale and time zone are read once per process (they are asked for on
//! every request and every subscription rebuild).

use std::sync::OnceLock;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;

const CHROME_VERSION: &str = "131.0.0.0";
const CLIENT_BUILD_NUMBER: u64 = 345678;

/// Client properties sent in IDENTIFY and, base64-encoded, as
/// `X-Super-Properties` on REST requests. Field names and order follow
/// what the official Discord web client (Chrome) sends.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ClientProperties {
    pub os: String,
    pub browser: String,
    pub device: String,
    pub system_locale: String,
    pub browser_user_agent: String,
    pub browser_version: String,
    pub os_version: String,
    pub referrer: String,
    pub referring_domain: String,
    pub referrer_current: String,
    pub referring_domain_current: String,
    pub release_channel: String,
    pub client_build_number: u64,
    pub client_event_source: Option<String>,
}

impl Default for ClientProperties {
    fn default() -> Self {
        current().clone()
    }
}

/// The properties of this process (computed once).
pub fn current() -> &'static ClientProperties {
    static PROPS: OnceLock<ClientProperties> = OnceLock::new();
    PROPS.get_or_init(|| ClientProperties {
        os: "Windows".into(),
        browser: "Chrome".into(),
        device: String::new(),
        system_locale: locale().into(),
        browser_user_agent: user_agent(),
        browser_version: CHROME_VERSION.into(),
        os_version: "10".into(),
        referrer: String::new(),
        referring_domain: String::new(),
        referrer_current: String::new(),
        referring_domain_current: String::new(),
        release_channel: "stable".into(),
        client_build_number: CLIENT_BUILD_NUMBER,
        client_event_source: None,
    })
}

/// Chrome's `User-Agent` string, matching the browser version above.
pub fn user_agent() -> String {
    format!(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/{CHROME_VERSION} Safari/537.36"
    )
}

/// Discord's spelling of an OS (`std::env::consts::OS` → `"Windows"`).
pub fn os_name(os: &str) -> &'static str {
    match os {
        "windows" => "Windows",
        "macos" => "Mac OS X",
        "linux" => "Linux",
        "freebsd" => "FreeBSD",
        "openbsd" => "OpenBSD",
        "netbsd" => "NetBSD",
        _ => "Unknown",
    }
}

/// `X-Super-Properties`: base64(JSON) of [`ClientProperties`].
pub fn super_properties(props: &ClientProperties) -> String {
    STANDARD.encode(serde_json::to_vec(props).unwrap_or_default())
}

/// The system UI language as a Discord locale (`en-US`), `en-US` if unknown.
pub fn locale() -> &'static str {
    static LOCALE: OnceLock<String> = OnceLock::new();
    LOCALE.get_or_init(|| normalize_locale(sys_locale::get_locale().as_deref()))
}

/// Turns `de_DE.UTF-8`, `pt-BR`, `C`, `es-419@euro` into a BCP-47-ish tag
/// Discord understands; anything unusable becomes `en-US`.
pub fn normalize_locale(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return "en-US".into();
    };
    // Strip the encoding and modifier (`.UTF-8`, `@euro`).
    let base = raw.split(['.', '@']).next().unwrap_or("").trim();
    let mut parts = base.split(['_', '-']).filter(|p| !p.is_empty());
    let Some(lang) = parts.next() else {
        return "en-US".into();
    };
    if !(2..=3).contains(&lang.len()) || !lang.chars().all(|c| c.is_ascii_alphabetic()) {
        return "en-US".into();
    }
    let lang = lang.to_ascii_lowercase();
    // The first 2-letter (or 3-digit) subtag is the region.
    match parts.find(|p| {
        (p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
            || (p.len() == 3 && p.chars().all(|c| c.is_ascii_digit()))
    }) {
        Some(region) => format!("{lang}-{}", region.to_ascii_uppercase()),
        None => lang,
    }
}

/// The IANA time zone (`Europe/Madrid`), `None` if it cannot be read.
pub fn timezone() -> Option<&'static str> {
    static TZ: OnceLock<Option<String>> = OnceLock::new();
    TZ.get_or_init(|| {
        iana_time_zone::get_timezone()
            .ok()
            .filter(|tz| valid_timezone(tz))
    })
    .as_deref()
}

/// A time zone name safe to put in a header (`Area/City`, `UTC`).
pub fn valid_timezone(tz: &str) -> bool {
    !tz.is_empty()
        && tz.len() <= 64
        && tz
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '+'))
}

/// Kernel release for the real OS, used when the client needs to identify
/// honestly (e.g. the update helper). Not called by `current()` because the
/// Chrome identity reports a fixed `"10"`.
#[allow(dead_code)]
fn os_version() -> String {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|s| s.trim().chars().take(64).collect())
            .unwrap_or_default()
    }
    #[cfg(not(target_os = "linux"))]
    {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_are_normalised() {
        assert_eq!(normalize_locale(Some("de_DE.UTF-8")), "de-DE");
        assert_eq!(normalize_locale(Some("pt-BR")), "pt-BR");
        assert_eq!(normalize_locale(Some("es-419")), "es-419");
        assert_eq!(normalize_locale(Some("fr")), "fr");
        assert_eq!(normalize_locale(Some("zh-Hans-CN")), "zh-CN");
        assert_eq!(normalize_locale(Some("ca_ES@valencia")), "ca-ES");
        assert_eq!(normalize_locale(Some("C")), "en-US");
        assert_eq!(normalize_locale(Some("POSIX")), "en-US");
        assert_eq!(normalize_locale(Some("")), "en-US");
        assert_eq!(normalize_locale(None), "en-US");
        assert_eq!(normalize_locale(Some("e\r\nvil")), "en-US");
    }

    #[test]
    fn timezones_are_header_safe() {
        assert!(valid_timezone("Europe/Madrid"));
        assert!(valid_timezone("America/Argentina/Buenos_Aires"));
        assert!(valid_timezone("Etc/GMT+3"));
        assert!(!valid_timezone(""));
        assert!(!valid_timezone("Europe/Madrid\r\nX-Evil: 1"));
        assert!(!valid_timezone(&"A".repeat(65)));
    }

    #[test]
    fn os_names_use_discord_spelling() {
        assert_eq!(os_name("windows"), "Windows");
        assert_eq!(os_name("linux"), "Linux");
        assert_eq!(os_name("macos"), "Mac OS X");
        assert_eq!(os_name("haiku"), "Unknown");
    }

    #[test]
    fn super_properties_decode_to_the_identify_properties() {
        let p = current();
        let raw = STANDARD.decode(super_properties(p)).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(v["browser"], "Chrome");
        assert_eq!(v["os"], "Windows");
        assert_eq!(v["browser_version"], CHROME_VERSION);
        assert_eq!(v["system_locale"], locale());
        assert_eq!(v["release_channel"], "stable");
        assert_eq!(v["client_build_number"], CLIENT_BUILD_NUMBER);
        assert!(v["client_event_source"].is_null());
        assert_eq!(v["browser_user_agent"], user_agent());
        // Same object as IDENTIFY's `properties`.
        assert_eq!(&ClientProperties::default(), p);
    }

    #[test]
    fn locale_and_timezone_are_cached_and_valid() {
        let l = locale();
        assert_eq!(normalize_locale(Some(l)), l);
        if let Some(tz) = timezone() {
            assert!(valid_timezone(tz));
        }
        assert!(std::ptr::eq(locale(), locale()));
    }
}
