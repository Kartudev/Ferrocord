//! The GitHub "latest release" document, asset selection and `.sha256`
//! files.

use serde::{Deserialize, Serialize};

use super::CRATE;
use super::version::{Version, is_newer};

/// Largest release document accepted (GitHub's are a few KB; generated
/// notes can make them longer).
pub const MAX_RELEASE_JSON: usize = 1024 * 1024;
/// Largest `.sha256` file accepted.
pub const MAX_SHA_FILE: u64 = 4096;

/// What the updater keeps from a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    /// `v0.18.0`.
    pub tag: String,
    pub version: Version,
    /// The release page (release notes).
    pub html_url: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Why a release document was not usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseError {
    Malformed,
    /// Drafts and pre-releases are never offered.
    NotARelease,
    /// The tag is not a plain `vMAJOR.MINOR.PATCH`.
    BadTag,
}

impl Release {
    /// Parses GitHub's `releases/latest` JSON.
    pub fn parse(body: &[u8]) -> Result<Self, ReleaseError> {
        if body.len() > MAX_RELEASE_JSON {
            return Err(ReleaseError::Malformed);
        }
        let raw: RawRelease = serde_json::from_slice(body).map_err(|_| ReleaseError::Malformed)?;
        if raw.draft || raw.prerelease {
            return Err(ReleaseError::NotARelease);
        }
        let version = Version::parse(&raw.tag_name).ok_or(ReleaseError::BadTag)?;
        Ok(Self {
            tag: raw.tag_name,
            version,
            html_url: raw.html_url,
            assets: raw
                .assets
                .into_iter()
                .map(|a| Asset {
                    name: a.name,
                    url: a.browser_download_url,
                    size: a.size,
                })
                .collect(),
        })
    }

    /// Newer than the running build?
    pub fn is_newer_than(&self, current: Version) -> bool {
        is_newer(current, &self.tag)
    }

    /// The release page, falling back to the repository's releases list
    /// when the document had none (or a non-GitHub one).
    pub fn page(&self) -> String {
        if self.html_url.starts_with("https://github.com/") {
            self.html_url.clone()
        } else {
            super::releases_page()
        }
    }

    /// The archive and checksum for `platform`, by their exact names
    /// (`release.yml`'s `ferrocord-v<ver>-<os>-x86_64.<ext>` + `.sha256`).
    pub fn assets_for(&self, platform: Platform) -> Option<(&Asset, &Asset)> {
        let name = platform.archive_name(self.version)?;
        let sum = format!("{name}.sha256");
        let archive = self.assets.iter().find(|a| a.name == name)?;
        let checksum = self.assets.iter().find(|a| a.name == sum)?;
        Some((archive, checksum))
    }
}

/// The platforms releases are built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    LinuxX64,
    WindowsX64,
    /// No release asset (other OS or CPU): notify only.
    Other,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Self::LinuxX64
        } else if cfg!(all(windows, target_arch = "x86_64")) {
            Self::WindowsX64
        } else {
            Self::Other
        }
    }

    /// Asset name of the release archive for `version`.
    pub fn archive_name(self, version: Version) -> Option<String> {
        match self {
            Self::LinuxX64 => Some(format!("{CRATE}-v{version}-linux-x86_64.tar.gz")),
            Self::WindowsX64 => Some(format!("{CRATE}-v{version}-windows-x86_64.zip")),
            Self::Other => None,
        }
    }
}

/// Reads a `sha256sum`-style line (`<64 hex>  <name>`, `*name` for binary
/// mode, CRLF tolerated) and returns the digest. A file name, when
/// present, must be `archive`.
pub fn parse_sha256(text: &str, archive: &str) -> Option<[u8; 32]> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut parts = line.split_whitespace();
    let hex = parts.next()?;
    if let Some(name) = parts.next() {
        let name = name.strip_prefix('*').unwrap_or(name);
        if name != archive || parts.next().is_some() {
            return None;
        }
    }
    decode_hex32(hex)
}

fn decode_hex32(hex: &str) -> Option<[u8; 32]> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in bytes.chunks(2).enumerate() {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out[i] = (hi * 16 + lo) as u8;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(tag: &str, extra: &str) -> String {
        format!(
            r#"{{"tag_name":"{tag}","html_url":"https://github.com/FerroApps/Ferrocord/releases/tag/{tag}",
               "draft":false,"prerelease":false{extra},
               "assets":[
                 {{"name":"ferrocord-v0.18.0-linux-x86_64.tar.gz","browser_download_url":"https://github.com/FerroApps/Ferrocord/releases/download/v0.18.0/ferrocord-v0.18.0-linux-x86_64.tar.gz","size":123}},
                 {{"name":"ferrocord-v0.18.0-linux-x86_64.tar.gz.sha256","browser_download_url":"https://github.com/x/s1","size":98}},
                 {{"name":"ferrocord-v0.18.0-windows-x86_64.zip","browser_download_url":"https://github.com/x/w","size":456}},
                 {{"name":"ferrocord-v0.18.0-windows-x86_64.zip.sha256","browser_download_url":"https://github.com/x/s2","size":95}}
               ]}}"#
        )
    }

    #[test]
    fn parses_the_latest_release() {
        let r = Release::parse(doc("v0.18.0", "").as_bytes()).unwrap();
        assert_eq!(r.tag, "v0.18.0");
        assert_eq!(r.version, Version::parse("0.18.0").unwrap());
        assert_eq!(r.assets.len(), 4);
        assert!(r.is_newer_than(Version::parse("0.17.1").unwrap()));
        assert!(!r.is_newer_than(Version::parse("0.18.0").unwrap()));
        assert!(r.page().ends_with("/tag/v0.18.0"));
    }

    #[test]
    fn ignores_drafts_prereleases_and_bad_tags() {
        let d = doc("v0.18.0", "").replace(r#""draft":false"#, r#""draft":true"#);
        assert_eq!(Release::parse(d.as_bytes()), Err(ReleaseError::NotARelease));
        let p = doc("v0.18.0", "").replace(r#""prerelease":false"#, r#""prerelease":true"#);
        assert_eq!(Release::parse(p.as_bytes()), Err(ReleaseError::NotARelease));
        assert_eq!(
            Release::parse(doc("v0.18.0-rc.1", "").as_bytes()),
            Err(ReleaseError::BadTag)
        );
        assert_eq!(
            Release::parse(doc("nightly", "").as_bytes()),
            Err(ReleaseError::BadTag)
        );
        assert_eq!(Release::parse(b"not json"), Err(ReleaseError::Malformed));
        assert_eq!(Release::parse(b"{}"), Err(ReleaseError::Malformed));
        let huge = vec![b' '; MAX_RELEASE_JSON + 1];
        assert_eq!(Release::parse(&huge), Err(ReleaseError::Malformed));
    }

    #[test]
    fn a_non_github_page_falls_back_to_the_releases_list() {
        let d = doc("v0.18.0", "").replace("https://github.com/FerroApps", "javascript:alert");
        let r = Release::parse(d.as_bytes()).unwrap();
        assert_eq!(r.page(), crate::update::releases_page());
    }

    #[test]
    fn selects_the_assets_of_the_platform_by_exact_name() {
        let r = Release::parse(doc("v0.18.0", "").as_bytes()).unwrap();
        let (a, s) = r.assets_for(Platform::LinuxX64).unwrap();
        assert_eq!(a.name, "ferrocord-v0.18.0-linux-x86_64.tar.gz");
        assert_eq!(s.name, "ferrocord-v0.18.0-linux-x86_64.tar.gz.sha256");
        assert_eq!(a.size, 123);
        let (a, s) = r.assets_for(Platform::WindowsX64).unwrap();
        assert_eq!(a.name, "ferrocord-v0.18.0-windows-x86_64.zip");
        assert!(s.name.ends_with(".zip.sha256"));
        assert!(r.assets_for(Platform::Other).is_none());
    }

    #[test]
    fn asset_selection_needs_the_checksum_and_the_right_version() {
        let mut r = Release::parse(doc("v0.18.0", "").as_bytes()).unwrap();
        r.assets
            .retain(|a| !a.name.ends_with("linux-x86_64.tar.gz.sha256"));
        assert!(r.assets_for(Platform::LinuxX64).is_none());
        // Assets of another version (a mis-tagged release) are not used.
        let r = Release::parse(doc("v0.19.0", "").as_bytes()).unwrap();
        assert!(r.assets_for(Platform::LinuxX64).is_none());
    }

    #[test]
    fn parses_sha256sum_lines() {
        let hex = "ab".repeat(32);
        let want = [0xabu8; 32];
        let name = "ferrocord-v0.18.0-linux-x86_64.tar.gz";
        assert_eq!(parse_sha256(&format!("{hex}  {name}\n"), name), Some(want));
        assert_eq!(
            parse_sha256(&format!("{hex} *{name}\r\n"), name),
            Some(want)
        );
        assert_eq!(parse_sha256(&hex.to_uppercase(), name), Some(want));
        assert_eq!(parse_sha256(&format!("\n{hex}\n"), name), Some(want));
        // Wrong file, short digest, junk.
        assert_eq!(parse_sha256(&format!("{hex}  other.tar.gz"), name), None);
        assert_eq!(parse_sha256(&format!("{hex}  {name} extra"), name), None);
        assert_eq!(parse_sha256(&"ab".repeat(31), name), None);
        assert_eq!(parse_sha256(&"zz".repeat(32), name), None);
        assert_eq!(parse_sha256("", name), None);
    }
}
