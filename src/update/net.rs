//! The one HTTP request of a check, and release downloads.
//!
//! HTTPS only (plain `http://` for loopback test servers named by
//! `FERRO_RELEASE_API`), redirects only to GitHub's download hosts, a size
//! cap and timeouts. No cookies, no account data, no machine id: just a
//! `User-Agent: ferrocord/<version>`.

use std::time::Duration;

use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::UpdateError;
use super::release::{MAX_RELEASE_JSON, MAX_SHA_FILE, Release};

/// Largest archive downloaded.
pub const MAX_DOWNLOAD: u64 = 300 * 1024 * 1024;
/// Hosts a download may be redirected to.
pub const REDIRECT_HOSTS: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];
/// Hosts an asset URL may start on (the API and the redirect targets).
const ASSET_HOSTS: [&str; 4] = [
    "github.com",
    "api.github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// `scheme://host/path`: GitHub asset URLs carry signed query strings,
/// which never go into logs.
pub fn redact_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(u) => format!(
            "{}://{}{}",
            u.scheme(),
            u.host_str().unwrap_or(""),
            u.path()
        ),
        Err(_) => "<invalid url>".to_owned(),
    }
}

fn is_loopback(u: &url::Url) -> bool {
    matches!(
        u.host(),
        Some(url::Host::Domain("localhost"))
            | Some(url::Host::Ipv4(std::net::Ipv4Addr::LOCALHOST))
            | Some(url::Host::Ipv6(std::net::Ipv6Addr::LOCALHOST))
    )
}

/// Whether the updater may fetch `url`: HTTPS to one of `hosts`, or
/// plain HTTP to loopback when `allow_loopback` (tests).
pub fn url_allowed(url: &url::Url, hosts: &[&str], allow_loopback: bool) -> bool {
    match url.scheme() {
        "https" => {
            url.host_str().is_some_and(|h| hosts.contains(&h))
                || (allow_loopback && is_loopback(url))
        }
        "http" => allow_loopback && is_loopback(url),
        _ => false,
    }
}

/// Where release information comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The full `releases/latest` URL.
    pub api_url: String,
    /// A loopback override (`FERRO_RELEASE_API`): loopback asset URLs are
    /// then allowed too.
    pub loopback: bool,
}

impl Endpoint {
    /// GitHub, or `FERRO_RELEASE_API` (the same variable and meaning as in
    /// `install.sh`: the full "latest release" URL) when it is an HTTPS
    /// URL or a loopback HTTP URL.
    pub fn from_env() -> Self {
        Self::resolve(std::env::var("FERRO_RELEASE_API").ok().as_deref())
    }

    pub fn resolve(over: Option<&str>) -> Self {
        let default = Self {
            api_url: super::api_url(),
            loopback: false,
        };
        let Some(raw) = over.map(str::trim).filter(|s| !s.is_empty()) else {
            return default;
        };
        match url::Url::parse(raw) {
            Ok(u) if u.scheme() == "https" && u.host_str().is_some() => Self {
                api_url: raw.to_owned(),
                loopback: is_loopback(&u),
            },
            Ok(u) if u.scheme() == "http" && is_loopback(&u) => Self {
                api_url: raw.to_owned(),
                loopback: true,
            },
            _ => {
                tracing::warn!("FERRO_RELEASE_API ignored (needs https or a loopback http URL)");
                default
            }
        }
    }
}

/// The HTTP client of the updater (built per check: nothing is kept in
/// memory between checks).
pub fn client(
    endpoint: &Endpoint,
    total_timeout: Duration,
) -> Result<reqwest::Client, UpdateError> {
    let loopback = endpoint.loopback;
    let policy = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= 5 {
            attempt.error("too many redirects")
        } else if url_allowed(attempt.url(), &REDIRECT_HOSTS, loopback) {
            attempt.follow()
        } else {
            tracing::warn!(to = %redact_url(attempt.url().as_str()), "update: redirect refused");
            attempt.stop()
        }
    });
    reqwest::Client::builder()
        .user_agent(format!("{}/{}", super::CRATE, env!("CARGO_PKG_VERSION")))
        .redirect(policy)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .timeout(total_timeout)
        .https_only(!loopback)
        .build()
        .map_err(|e| UpdateError::Network(e.to_string()))
}

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    /// `304 Not Modified`: the cached release still stands.
    NotModified,
    Release {
        release: Release,
        etag: Option<String>,
    },
    /// The latest release is a draft / pre-release / odd tag: nothing to
    /// offer.
    NoRelease,
    /// 403 / 429: back off for this long.
    RateLimited(Option<Duration>),
}

/// `GET releases/latest` with `If-None-Match`.
pub async fn check(endpoint: &Endpoint, etag: Option<&str>) -> Result<CheckOutcome, UpdateError> {
    let c = client(endpoint, Duration::from_secs(30))?;
    let mut req = c
        .get(&endpoint.api_url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(tag) = etag {
        req = req.header(reqwest::header::IF_NONE_MATCH, tag);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| UpdateError::Network(e.without_url().to_string()))?;
    let status = resp.status().as_u16();
    match status {
        304 => return Ok(CheckOutcome::NotModified),
        403 | 429 => return Ok(CheckOutcome::RateLimited(retry_after(resp.headers()))),
        // No release published yet.
        404 => return Ok(CheckOutcome::NoRelease),
        200 => {}
        s => return Err(UpdateError::Http(s)),
    }
    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.len() < 200)
        .map(str::to_owned);
    let body = read_capped(resp, MAX_RELEASE_JSON as u64).await?;
    match Release::parse(&body) {
        Ok(release) => Ok(CheckOutcome::Release { release, etag }),
        Err(super::release::ReleaseError::Malformed) => {
            Err(UpdateError::Network("malformed release information".into()))
        }
        Err(_) => Ok(CheckOutcome::NoRelease),
    }
}

/// `retry-after` (seconds) or `x-ratelimit-reset` (epoch seconds).
pub fn retry_after(h: &reqwest::header::HeaderMap) -> Option<Duration> {
    let num = |k: &str| {
        h.get(k)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
    };
    if let Some(s) = num("retry-after") {
        return Some(Duration::from_secs(s));
    }
    let reset = num("x-ratelimit-reset")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(Duration::from_secs(reset.saturating_sub(now)))
}

async fn read_capped(resp: reqwest::Response, cap: u64) -> Result<Vec<u8>, UpdateError> {
    if resp.content_length().is_some_and(|n| n > cap) {
        return Err(UpdateError::TooLarge);
    }
    let mut out = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| UpdateError::Network(e.without_url().to_string()))?;
        if out.len() as u64 + chunk.len() as u64 > cap {
            return Err(UpdateError::TooLarge);
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

fn checked_asset_url(url: &str, endpoint: &Endpoint) -> Result<url::Url, UpdateError> {
    let u = url::Url::parse(url).map_err(|_| UpdateError::Insecure("invalid URL".into()))?;
    if url_allowed(&u, &ASSET_HOSTS, endpoint.loopback) {
        Ok(u)
    } else {
        Err(UpdateError::Insecure(
            u.host_str().unwrap_or("?").to_owned(),
        ))
    }
}

async fn get(c: &reqwest::Client, url: url::Url) -> Result<reqwest::Response, UpdateError> {
    let resp = c
        .get(url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await
        .map_err(|e| UpdateError::Network(e.without_url().to_string()))?;
    match resp.status().as_u16() {
        200 => Ok(resp),
        // A redirect we refused to follow.
        s @ 300..=399 => Err(UpdateError::Insecure(format!("redirect ({s})"))),
        s => Err(UpdateError::Http(s)),
    }
}

/// Downloads the small `.sha256` file.
pub async fn fetch_text(endpoint: &Endpoint, url: &str) -> Result<String, UpdateError> {
    let u = checked_asset_url(url, endpoint)?;
    tracing::info!(url = %redact_url(url), "update: downloading checksum");
    let c = client(endpoint, Duration::from_secs(60))?;
    let body = read_capped(get(&c, u).await?, MAX_SHA_FILE).await?;
    String::from_utf8(body).map_err(|_| UpdateError::Checksum)
}

/// Streams `url` into `dest` (a new file), calling `progress(received,
/// total)` as bytes arrive, and returns the SHA-256 of what was written.
/// Dropping the future cancels the download.
pub async fn download(
    endpoint: &Endpoint,
    url: &str,
    dest: &std::path::Path,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<[u8; 32], UpdateError> {
    let u = checked_asset_url(url, endpoint)?;
    tracing::info!(url = %redact_url(url), "update: downloading");
    let c = client(endpoint, Duration::from_secs(30 * 60))?;
    let resp = get(&c, u).await?;
    let total = resp.content_length();
    if total.is_some_and(|n| n > MAX_DOWNLOAD) {
        return Err(UpdateError::TooLarge);
    }
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .await
        .map_err(UpdateError::io)?;
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    let mut stream = resp.bytes_stream();
    progress(0, total);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| UpdateError::Network(e.without_url().to_string()))?;
        received += chunk.len() as u64;
        if received > MAX_DOWNLOAD {
            return Err(UpdateError::TooLarge);
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(UpdateError::io)?;
        progress(received, total);
    }
    if total.is_some_and(|n| n != received) {
        return Err(UpdateError::Network("download ended early".into()));
    }
    file.flush().await.map_err(UpdateError::io)?;
    file.sync_all().await.map_err(UpdateError::io)?;
    Ok(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_strips_signed_queries() {
        assert_eq!(
            redact_url(
                "https://release-assets.githubusercontent.com/x/y.tar.gz?sig=SECRET&X-Amz=1"
            ),
            "https://release-assets.githubusercontent.com/x/y.tar.gz"
        );
        assert_eq!(redact_url("::"), "<invalid url>");
    }

    #[test]
    fn only_github_hosts_over_https() {
        let ok = |s: &str, lb: bool| url_allowed(&url::Url::parse(s).unwrap(), &REDIRECT_HOSTS, lb);
        assert!(ok(
            "https://github.com/FerroApps/Ferrocord/releases/download/v1/x",
            false
        ));
        assert!(ok("https://objects.githubusercontent.com/a", false));
        assert!(ok("https://release-assets.githubusercontent.com/a", false));
        assert!(!ok("http://github.com/a", false));
        assert!(!ok("https://evil.example/a", false));
        assert!(!ok("https://github.com.evil.example/a", false));
        assert!(!ok("http://127.0.0.1:9/a", false));
        assert!(ok("http://127.0.0.1:9/a", true));
        assert!(!ok("http://10.0.0.1/a", true));
        assert!(!ok("file:///etc/passwd", true));
    }

    #[test]
    fn endpoint_override_follows_install_sh() {
        assert_eq!(Endpoint::resolve(None).api_url, crate::update::api_url());
        assert!(
            crate::update::api_url()
                .ends_with(&format!("/repos/{}/releases/latest", crate::update::repo()))
        );
        let e = Endpoint::resolve(Some("http://127.0.0.1:5000/latest.json"));
        assert!(e.loopback);
        assert_eq!(e.api_url, "http://127.0.0.1:5000/latest.json");
        let e = Endpoint::resolve(Some("https://mirror.example/latest"));
        assert!(!e.loopback);
        // Insecure or local-file overrides are ignored.
        for bad in [
            "http://example.com/x",
            "file:///tmp/latest.json",
            "nonsense",
        ] {
            assert_eq!(
                Endpoint::resolve(Some(bad)).api_url,
                crate::update::api_url()
            );
        }
    }

    #[test]
    fn retry_after_headers() {
        let mut h = reqwest::header::HeaderMap::new();
        assert_eq!(retry_after(&h), None);
        h.insert("retry-after", "120".parse().unwrap());
        assert_eq!(retry_after(&h), Some(Duration::from_secs(120)));
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("x-ratelimit-reset", "1".parse().unwrap());
        assert_eq!(retry_after(&h), Some(Duration::ZERO));
    }
}
