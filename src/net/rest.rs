//! REST API client (`reqwest`) with rate limiting.

use std::sync::Arc;
use std::time::Duration;

use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::time::Instant;

use super::extras::SearchQuery;
use super::model::*;
use super::ratelimit::{RateLimitHeaders, RateLimiter, route_key};
use super::types::{
    AllowedMentions, CreateMessage, MessageReference, RateLimitBody, WireMessage, WireUser,
};
use crate::auth::secrets::Secret;
use crate::error::{Error, Result};

mod ext;
mod friends;
pub mod guild;
mod settings;
pub use settings::ProfileEdit;
mod voice;
pub use voice::ring_body;

const MAX_RETRIES: u32 = 3;
/// Hard cap for downloaded media.
pub const MAX_MEDIA_BYTES: usize = 10 * 1024 * 1024;
/// Images above this many pixels are not handed to the renderer.
pub const MAX_MEDIA_PIXELS: u64 = 4096 * 4096;

/// Reads image dimensions from the header and rejects decompression bombs.
/// Reads a media response body, refusing errors and bodies larger than
/// [`MAX_MEDIA_BYTES`].
async fn read_capped(mut resp: reqwest::Response, what: &str) -> Result<Vec<u8>> {
    if !resp.status().is_success() {
        return Err(Error::Http {
            status: resp.status().as_u16(),
            message: what.into(),
        });
    }
    if resp
        .content_length()
        .is_some_and(|l| l > MAX_MEDIA_BYTES as u64)
    {
        return Err(Error::Protocol("media too large".into()));
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if out.len() + chunk.len() > MAX_MEDIA_BYTES {
            return Err(Error::Protocol("media too large".into()));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// One entry of `GET /gifs/trending-gifs` / `GET /gifs/search`:
/// `{id, title, url, src, gif_src, width, height, preview}`. `src` is the
/// file in the requested `media_format` (MP4), `gif_src` the GIF and
/// `preview` a still. Entries without a page URL or any image are dropped.
pub fn parse_gif(v: &Value) -> Option<Gif> {
    let s = |k: &str| v[k].as_str().unwrap_or_default().to_owned();
    let dim = |k: &str| {
        v[k].as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(1)
            .max(1)
    };
    let url = s("url");
    let (preview, gif_src) = (s("preview"), s("gif_src"));
    if url.is_empty() || (preview.is_empty() && gif_src.is_empty()) {
        return None;
    }
    Some(Gif {
        url,
        src: s("src"),
        preview: if preview.is_empty() {
            gif_src.clone()
        } else {
            preview
        },
        gif_src,
        width: dim("width"),
        height: dim("height"),
    })
}

pub fn check_image_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| Error::Protocol("unreadable image".into()))?;
    let (w, h) = reader
        .into_dimensions()
        .map_err(|_| Error::Protocol("unreadable image".into()))?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_MEDIA_PIXELS {
        return Err(Error::Protocol("image too large".into()));
    }
    Ok((w, h))
}

/// Cheaply clonable REST handle.
#[derive(Clone)]
pub struct RestClient {
    http: reqwest::Client,
    /// Plain client for third-party media the API pointed at (GIF picker
    /// files): no Discord headers, no cookies, like a browser `<img>`.
    media_http: reqwest::Client,
    base: Arc<str>,
    cdn: Arc<str>,
    token: Option<Secret>,
    limiter: Arc<Mutex<RateLimiter>>,
}

impl std::fmt::Debug for RestClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestClient")
            .field("base", &self.base)
            .field("authenticated", &self.token.is_some())
            .finish()
    }
}

pub use super::identity::user_agent;

/// Headers every REST request carries besides `User-Agent` (see
/// [`super::identity`]): the same set the official clients send.
pub fn client_headers() -> reqwest::header::HeaderMap {
    use reqwest::header::{HeaderMap, HeaderValue};
    let mut h = HeaderMap::new();
    let props = super::identity::current();
    let pairs = [
        (
            "X-Super-Properties",
            Some(super::identity::super_properties(props)),
        ),
        (
            "X-Discord-Locale",
            Some(super::identity::locale().to_owned()),
        ),
        (
            "X-Discord-Timezone",
            super::identity::timezone().map(String::from),
        ),
    ];
    for (name, value) in pairs {
        if let Some(v) = value.and_then(|v| HeaderValue::from_str(&v).ok()) {
            h.insert(name, v);
        }
    }
    h
}

impl RestClient {
    pub fn new(base: &str, cdn: &str, token: Option<Secret>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(user_agent())
            .default_headers(client_headers())
            // Cookies Discord sets (`__dcfduid`, `__sdcfduid`, `__cfruid`)
            // are sent back like a browser would; kept in memory only.
            .cookie_store(true)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        let media_http = reqwest::Client::builder()
            .user_agent(user_agent())
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self {
            http,
            media_http,
            base: base.trim_end_matches('/').into(),
            cdn: cdn.trim_end_matches('/').into(),
            token,
            limiter: Arc::new(Mutex::new(RateLimiter::new())),
        })
    }

    pub fn with_token(&self, token: Secret) -> Self {
        Self {
            token: Some(token),
            ..self.clone()
        }
    }

    pub fn cdn_base(&self) -> &str {
        &self.cdn
    }

    /// Sends a JSON request, waiting for rate limits and retrying 429s.
    async fn request(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        self.send_with(method, path, |req| match &body {
            Some(b) => req.json(b),
            None => req,
        })
        .await
    }

    /// Like [`request`](Self::request) with a custom body: `build` is called
    /// for every attempt (a 429 retry needs a fresh body).
    async fn send_with(
        &self,
        method: Method,
        path: &str,
        build: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<Value> {
        let route = route_key(method.as_str(), path);
        let url = format!("{}{}", self.base, path);
        let mut attempt = 0;
        loop {
            // Wait for the bucket.
            loop {
                let wait = self.limiter.lock().await.acquire(&route, Instant::now());
                match wait {
                    Some(d) => {
                        tracing::debug!(route = %route, wait_ms = d.as_millis() as u64, "rate limited locally");
                        tokio::time::sleep(d).await;
                    }
                    None => break,
                }
            }
            let mut req = self.http.request(method.clone(), &url);
            if let Some(t) = &self.token {
                req = req.header(reqwest::header::AUTHORIZATION, t.expose());
            }
            req = build(req);
            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    self.limiter.lock().await.release(&route);
                    return Err(e.into());
                }
            };
            let status = resp.status();
            let headers = RateLimitHeaders::from_headers(resp.headers());
            let retry_after_header = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<f64>().ok());
            self.limiter
                .lock()
                .await
                .update(&route, &headers, Instant::now());

            if status == StatusCode::TOO_MANY_REQUESTS {
                let body: Option<RateLimitBody> = resp.json().await.ok();
                let secs = body
                    .as_ref()
                    .map(|b| b.retry_after)
                    .filter(|s| *s > 0.0)
                    .or(retry_after_header)
                    .unwrap_or(1.0);
                let global = headers.global || body.is_some_and(|b| b.global);
                let wait = super::ratelimit::secs(secs).min(Duration::from_secs(300));
                self.limiter
                    .lock()
                    .await
                    .on_429(&route, wait, global, Instant::now());
                attempt += 1;
                tracing::warn!(route = %route, retry_after_ms = wait.as_millis() as u64, global, "HTTP 429");
                if attempt > MAX_RETRIES {
                    return Err(Error::Http {
                        status: 429,
                        message: "rate limited".into(),
                    });
                }
                continue;
            }
            if status == StatusCode::NO_CONTENT {
                return Ok(Value::Null);
            }
            let text = resp.text().await?;
            if !status.is_success() {
                // Discord answers "captcha required" with a body that has
                // `captcha_key` instead of `message`. Ferrocord never solves
                // it; the caller decides what the user sees.
                if status == StatusCode::BAD_REQUEST
                    && let Some(c) = super::captcha::CaptchaChallenge::parse(&text)
                {
                    return Err(Error::CaptchaRequired(Box::new(c)));
                }
                // The error body has a `message` field; it is a server
                // description, not user content.
                let message = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v["message"].as_str().map(String::from))
                    .unwrap_or_else(|| status.canonical_reason().unwrap_or("error").into());
                return Err(Error::Http {
                    status: status.as_u16(),
                    message,
                });
            }
            if text.is_empty() {
                return Ok(Value::Null);
            }
            return Ok(serde_json::from_str(&text)?);
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        Ok(serde_json::from_value(
            self.request(Method::GET, path, None).await?,
        )?)
    }

    /// GIFs for the picker: trending when `query` is blank, else a search.
    /// `GET /gifs/trending-gifs` / `GET /gifs/search` (Tenor via Discord).
    pub async fn gifs(&self, query: &str) -> Result<Vec<Gif>> {
        let q = query.trim();
        let path = if q.is_empty() {
            "/gifs/trending-gifs?media_format=mp4&provider=tenor&locale=en-US".to_owned()
        } else {
            format!(
                "/gifs/search?q={}&media_format=mp4&provider=tenor&locale=en-US",
                percent_encode(q)
            )
        };
        let wire: Vec<Value> = self.get(&path).await?;
        Ok(wire.iter().filter_map(parse_gif).collect())
    }

    /// `GET /users/@me` — validates the token.
    pub async fn me(&self) -> Result<User> {
        let u: WireUser = self.get("/users/@me").await.map_err(|e| match e {
            Error::Http { status: 401, .. } => Error::Auth("invalid token".into()),
            other => other,
        })?;
        Ok(u.into())
    }

    /// `GET /channels/{id}/messages` — newest first on the wire; returned
    /// oldest first.
    pub async fn messages(
        &self,
        channel: ChannelId,
        before: Option<MessageId>,
        limit: u8,
    ) -> Result<Vec<Message>> {
        let mut path = format!("/channels/{channel}/messages?limit={}", limit.clamp(1, 100));
        if let Some(b) = before {
            path.push_str(&format!("&before={b}"));
        }
        let wire: Vec<WireMessage> = self.get(&path).await?;
        let mut msgs: Vec<Message> = wire.into_iter().map(WireMessage::into_domain).collect();
        msgs.sort_by_key(|m| m.id);
        Ok(msgs)
    }

    /// `POST /channels/{id}/messages`, optionally as a reply.
    pub async fn send_message(
        &self,
        channel: ChannelId,
        content: &str,
        nonce: &str,
        reply: Option<ReplyTarget>,
    ) -> Result<Message> {
        let body = serde_json::to_value(create_message_body(channel, content, nonce, reply))?;
        let v = self
            .request(
                Method::POST,
                &format!("/channels/{channel}/messages"),
                Some(body),
            )
            .await?;
        let wire: WireMessage = serde_json::from_value(v)?;
        Ok(wire.into_domain())
    }

    /// `POST /channels/{id}/messages` with only a sticker (`sticker_ids`).
    pub async fn send_sticker(
        &self,
        channel: ChannelId,
        sticker: Snowflake,
        nonce: &str,
        reply: Option<ReplyTarget>,
    ) -> Result<Message> {
        let mut body = create_message_body(channel, "", nonce, reply);
        body.sticker_ids = vec![sticker];
        let v = self
            .request(
                Method::POST,
                &format!("/channels/{channel}/messages"),
                Some(serde_json::to_value(body)?),
            )
            .await?;
        let wire: WireMessage = serde_json::from_value(v)?;
        Ok(wire.into_domain())
    }

    /// `PATCH /channels/{c}/messages/{m}` — edits the content of one of the
    /// user's own messages. Returns the updated message.
    pub async fn edit_message(
        &self,
        channel: ChannelId,
        message: MessageId,
        content: &str,
    ) -> Result<Message> {
        let v = self
            .request(
                Method::PATCH,
                &format!("/channels/{channel}/messages/{message}"),
                Some(serde_json::json!({ "content": content })),
            )
            .await?;
        Ok(serde_json::from_value::<WireMessage>(v)?.into_domain())
    }

    /// `PATCH /channels/{c}/messages/{m}` with `flags` — what the official
    /// client's embed "X" sends (`flags | SUPPRESS_EMBEDS`).
    pub async fn set_message_flags(
        &self,
        channel: ChannelId,
        message: MessageId,
        flags: u64,
    ) -> Result<Message> {
        let v = self
            .request(
                Method::PATCH,
                &format!("/channels/{channel}/messages/{message}"),
                Some(serde_json::json!({ "flags": flags })),
            )
            .await?;
        Ok(serde_json::from_value::<WireMessage>(v)?.into_domain())
    }

    /// `DELETE /channels/{c}/messages/{m}`.
    pub async fn delete_message(&self, channel: ChannelId, message: MessageId) -> Result<()> {
        self.request(
            Method::DELETE,
            &format!("/channels/{channel}/messages/{message}"),
            None,
        )
        .await
        .map(|_| ())
    }

    /// `PUT/DELETE /channels/{c}/messages/{m}/reactions/{emoji}/@me`.
    pub async fn set_reaction(
        &self,
        channel: ChannelId,
        message: MessageId,
        emoji: &Emoji,
        add: bool,
    ) -> Result<()> {
        let path = format!(
            "/channels/{channel}/messages/{message}/reactions/{}/@me",
            emoji.api_segment()
        );
        let method = if add { Method::PUT } else { Method::DELETE };
        self.request(method, &path, None).await.map(|_| ())
    }

    /// `GET /channels/{c}/messages?around={m}` — a window centred on a
    /// message (for jumps). Returned oldest first.
    pub async fn messages_around(
        &self,
        channel: ChannelId,
        around: MessageId,
        limit: u8,
    ) -> Result<Vec<Message>> {
        let path = format!(
            "/channels/{channel}/messages?limit={}&around={around}",
            limit.clamp(1, 100)
        );
        let wire: Vec<WireMessage> = self.get(&path).await?;
        let mut msgs: Vec<Message> = wire.into_iter().map(WireMessage::into_domain).collect();
        msgs.sort_by_key(|m| m.id);
        Ok(msgs)
    }

    /// `GET /channels/{c}/messages?after={m}` — newer messages, oldest first.
    pub async fn messages_after(
        &self,
        channel: ChannelId,
        after: MessageId,
        limit: u8,
    ) -> Result<Vec<Message>> {
        let path = format!(
            "/channels/{channel}/messages?limit={}&after={after}",
            limit.clamp(1, 100)
        );
        let wire: Vec<WireMessage> = self.get(&path).await?;
        let mut msgs: Vec<Message> = wire.into_iter().map(WireMessage::into_domain).collect();
        msgs.sort_by_key(|m| m.id);
        Ok(msgs)
    }

    /// `PATCH /users/@me/settings` — persists the user's status (the
    /// official client does the same; the change is pushed to all sessions).
    pub async fn set_status_setting(&self, status: Presence) -> Result<()> {
        self.request(
            Method::PATCH,
            "/users/@me/settings",
            Some(serde_json::json!({ "status": status.wire() })),
        )
        .await
        .map(|_| ())
    }

    /// Message search in a guild (`guild = Some`) or a DM channel.
    /// Discord answers 202 while it builds the index; that is retried a few
    /// times, honouring `retry_after`.
    pub async fn search(&self, scope: SearchScope, query: &str, offset: u32) -> Result<SearchPage> {
        self.search_query(scope, &SearchQuery::text(query), offset)
            .await
    }

    /// Search with `from:` / `has:` / `in:` filters.
    pub async fn search_query(
        &self,
        scope: SearchScope,
        query: &SearchQuery,
        offset: u32,
    ) -> Result<SearchPage> {
        let path = search_path_q(scope, query, offset);
        for attempt in 0..4 {
            let v = self.request(Method::GET, &path, None).await?;
            if let Some(wait) = search_not_ready(&v) {
                if attempt == 3 {
                    return Err(Error::Protocol("search index not ready".into()));
                }
                tokio::time::sleep(wait).await;
                continue;
            }
            return parse_search(v, scope.guild());
        }
        Err(Error::Protocol("search index not ready".into()))
    }

    /// `POST /channels/{id}/messages` with `multipart/form-data`: the
    /// caption and reply go in `payload_json`, each file is streamed from
    /// disk (never fully buffered). `progress` receives the number of body
    /// bytes handed to the socket so far.
    pub async fn send_with_files(
        &self,
        channel: ChannelId,
        content: &str,
        nonce: &str,
        reply: Option<ReplyTarget>,
        files: &[UploadFile],
        progress: tokio::sync::mpsc::UnboundedSender<u64>,
    ) -> Result<Message> {
        let mut payload =
            serde_json::to_value(create_message_body(channel, content, nonce, reply))?;
        payload["attachments"] = serde_json::Value::Array(
            files
                .iter()
                .enumerate()
                .map(|(i, f)| serde_json::json!({ "id": i, "filename": f.name }))
                .collect(),
        );
        let payload = payload.to_string();
        let boundary = multipart_boundary();
        let layout = MultipartLayout::new(&boundary, &payload, files);
        let path = format!("/channels/{channel}/messages");
        let v = self
            .send_with(Method::POST, &path, |req| {
                let body = multipart_body(&layout, files, progress.clone());
                req.header(
                    reqwest::header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .header(reqwest::header::CONTENT_LENGTH, layout.total_len)
                .body(body)
            })
            .await?;
        Ok(serde_json::from_value::<WireMessage>(v)?.into_domain())
    }

    /// `POST /channels/{id}/typing`.
    pub async fn trigger_typing(&self, channel: ChannelId) -> Result<()> {
        self.request(Method::POST, &format!("/channels/{channel}/typing"), None)
            .await
            .map(|_| ())
    }

    /// `POST /channels/{c}/messages/{m}/ack` — marks the channel read.
    pub async fn ack(&self, channel: ChannelId, message: MessageId) -> Result<()> {
        self.request(
            Method::POST,
            &format!("/channels/{channel}/messages/{message}/ack"),
            Some(serde_json::json!({ "token": null })),
        )
        .await
        .map(|_| ())
    }

    /// `POST /users/@me/remote-auth/login` — exchanges a QR-login ticket for
    /// the RSA-encrypted token.
    pub async fn remote_auth_login(&self, ticket: &str) -> Result<String> {
        self.remote_auth_login_with(ticket, None, None).await
    }

    /// The ticket exchange as the official client sends it: with the
    /// `X-Fingerprint` from [`fingerprint`](Self::fingerprint) and, when
    /// retrying after a captcha the user solved, the captcha headers.
    pub async fn remote_auth_login_with(
        &self,
        ticket: &str,
        fingerprint: Option<&str>,
        captcha: Option<&super::captcha::CaptchaSolution>,
    ) -> Result<String> {
        let body = serde_json::json!({ "ticket": ticket });
        let fingerprint = fingerprint.filter(|f| super::captcha::header_safe(f));
        let v = self
            .send_with(Method::POST, "/users/@me/remote-auth/login", |mut req| {
                if let Some(f) = fingerprint {
                    req = req.header("X-Fingerprint", f);
                }
                if let Some(c) = captcha {
                    for (k, v) in c.headers() {
                        req = req.header(k, v);
                    }
                }
                req.json(&body)
            })
            .await?;
        v["encrypted_token"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| Error::Protocol("remote-auth login without encrypted_token".into()))
    }

    /// `GET /experiments` while logged out: the official client fetches it
    /// first and sends its `fingerprint` as `X-Fingerprint` on the login
    /// calls. The response also sets Discord's `__dcfduid`/`__sdcfduid`
    /// cookies. `None` if the call fails or the value is unusable.
    pub async fn fingerprint(&self) -> Option<String> {
        let v = self.request(Method::GET, "/experiments", None).await.ok()?;
        v["fingerprint"]
            .as_str()
            .filter(|f| f.len() <= 128 && super::captcha::header_safe(f))
            .map(String::from)
    }

    /// Whether `url` points at Discord's CDN / media proxy (or the
    /// configured CDN override). Nothing else is ever fetched: embeds
    /// without a proxied URL are skipped instead of contacting arbitrary
    /// third-party hosts.
    pub fn is_allowed_media_url(&self, url: &str) -> bool {
        let Ok(u) = url::Url::parse(url) else {
            return false;
        };
        let Some(host) = u.host_str() else {
            return false;
        };
        let cdn_host = url::Url::parse(&self.cdn)
            .ok()
            .and_then(|c| c.host_str().map(str::to_owned));
        if cdn_host.as_deref() == Some(host) {
            // Override (e.g. a local mock): same scheme as configured.
            return url.starts_with(self.cdn.as_ref());
        }
        // Tenor serves the GIF picker's previews (only exact host; these
        // downloads never carry the token, see `request`).
        u.scheme() == "https"
            && (matches!(
                host,
                "cdn.discordapp.com" | "media.discordapp.net" | "media.tenor.com" | "i.scdn.co"
            ) || (host.starts_with("images-ext-") && host.ends_with(".discordapp.net")))
    }

    /// Downloads a GIF picker file (preview or GIF) at a URL Discord's GIF
    /// API returned. The official client loads these straight from the
    /// provider's CDN, whatever its host (Tenor moved between
    /// `media.tenor.com`, `media1.tenor.com`, `c.tenor.com`), so any HTTPS
    /// host is allowed here, but only through the plain media client: no
    /// token, no Discord headers or cookies, and the usual size cap.
    pub async fn fetch_picker_media(&self, url: &str) -> Result<Vec<u8>> {
        if self.is_allowed_media_url(url) {
            return self.fetch_bytes(url).await;
        }
        let ok =
            url::Url::parse(url).is_ok_and(|u| u.scheme() == "https" && u.host_str().is_some());
        if !ok {
            return Err(Error::Protocol("media host not allowed".into()));
        }
        let resp = self.media_http.get(url).send().await?;
        read_capped(resp, "gif download failed").await
    }

    /// Downloads a CDN asset (avatar, icon, attachment preview), refusing
    /// non-Discord hosts and anything larger than [`MAX_MEDIA_BYTES`].
    pub async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>> {
        if !self.is_allowed_media_url(url) {
            return Err(Error::Protocol("media host not allowed".into()));
        }
        let host_is_discord = url::Url::parse(url)
            .ok()
            .and_then(|u| {
                u.host_str()
                    .map(|h| h.ends_with("discordapp.com") || h.ends_with("discordapp.net"))
            })
            .unwrap_or(false);
        // Third-party hosts (Tenor, Spotify art) get the plain client.
        let client = if host_is_discord || url.starts_with(self.cdn.as_ref()) {
            &self.http
        } else {
            &self.media_http
        };
        let resp = client.get(url).send().await?;
        read_capped(resp, "asset download failed").await
    }

    /// Streams a CDN file into `file` (a video, too big to hold in memory),
    /// refusing non-Discord hosts, redirects to other hosts and anything
    /// larger than `max` bytes. No token is sent. `progress(received,
    /// total)` is called after every few hundred KB. Returns the size.
    pub async fn download_file(
        &self,
        url: &str,
        file: std::fs::File,
        max: u64,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        use std::io::Write;
        if !self.is_allowed_media_url(url) {
            return Err(Error::Protocol("media host not allowed".into()));
        }
        let mut resp = self.http.get(url).send().await?;
        if !self.is_allowed_media_url(resp.url().as_str()) {
            return Err(Error::Protocol("media host not allowed".into()));
        }
        if !resp.status().is_success() {
            return Err(Error::Http {
                status: resp.status().as_u16(),
                message: "video download failed".into(),
            });
        }
        let total = resp.content_length();
        if total.is_some_and(|l| l > max) {
            return Err(Error::Protocol("media too large".into()));
        }
        let mut out = std::io::BufWriter::with_capacity(256 * 1024, file);
        let mut received = 0u64;
        let mut reported = 0u64;
        while let Some(chunk) = resp.chunk().await? {
            received += chunk.len() as u64;
            if received > max {
                return Err(Error::Protocol("media too large".into()));
            }
            out.write_all(&chunk)
                .map_err(|_| Error::Protocol("cannot write the video".into()))?;
            if received - reported >= 256 * 1024 {
                reported = received;
                progress(received, total);
            }
        }
        out.flush()
            .map_err(|_| Error::Protocol("cannot write the video".into()))?;
        progress(received, total);
        Ok(received)
    }

    pub fn avatar_url(&self, user: &User, size: u32) -> Option<String> {
        user.avatar
            .as_ref()
            .map(|h| format!("{}/avatars/{}/{h}.png?size={size}", self.cdn, user.id))
    }

    pub fn guild_icon_url(&self, guild: &Guild, size: u32) -> Option<String> {
        guild
            .icon
            .as_ref()
            .map(|h| format!("{}/icons/{}/{h}.png?size={size}", self.cdn, guild.id))
    }
}

// ---------------------------------------------------------------------------
// Replies, search and multipart helpers (pure, unit-tested)
// ---------------------------------------------------------------------------

/// What a reply points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplyTarget {
    pub message: MessageId,
    pub guild: Option<GuildId>,
    /// "@ ON" in the reply bar.
    pub mention: bool,
}

pub fn create_message_body<'a>(
    channel: ChannelId,
    content: &'a str,
    nonce: &'a str,
    reply: Option<ReplyTarget>,
) -> CreateMessage<'a> {
    CreateMessage {
        content,
        nonce,
        tts: false,
        message_reference: reply.map(|r| MessageReference {
            message_id: r.message,
            channel_id: channel,
            guild_id: r.guild,
            fail_if_not_exists: false,
        }),
        allowed_mentions: reply.filter(|r| !r.mention).map(|_| AllowedMentions {
            parse: vec!["users", "roles", "everyone"],
            replied_user: false,
        }),
        sticker_ids: Vec::new(),
    }
}

/// Where to search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchScope {
    Guild(GuildId),
    Channel(ChannelId),
}

impl SearchScope {
    pub fn guild(self) -> Option<GuildId> {
        match self {
            Self::Guild(g) => Some(g),
            Self::Channel(_) => None,
        }
    }
}

/// Results per page returned by the search endpoint.
pub const SEARCH_PAGE: u32 = 25;

pub fn search_path(scope: SearchScope, query: &str, offset: u32) -> String {
    search_path_q(scope, &SearchQuery::text(query), offset)
}

/// Search URL with filters: `content`, then `author_id`, `has` and
/// `channel_id` (each repeated per value; `channel_id` only for server
/// searches), then paging.
pub fn search_path_q(scope: SearchScope, q: &SearchQuery, offset: u32) -> String {
    let base = match scope {
        SearchScope::Guild(g) => format!("/guilds/{g}/messages/search"),
        SearchScope::Channel(c) => format!("/channels/{c}/messages/search"),
    };
    let mut params: Vec<String> = Vec::new();
    if !q.content.is_empty() {
        params.push(format!("content={}", percent_encode(&q.content)));
    }
    params.extend(q.authors.iter().map(|a| format!("author_id={a}")));
    params.extend(q.has.iter().map(|h| format!("has={h}")));
    if scope.guild().is_some() {
        params.extend(q.channels.iter().map(|c| format!("channel_id={c}")));
    }
    params.push(format!("offset={offset}"));
    params.push("include_nsfw=true".into());
    format!("{base}?{}", params.join("&"))
}

/// `Some(wait)` when the response means "index not ready, retry".
pub fn search_not_ready(v: &Value) -> Option<Duration> {
    let code = v.get("code").and_then(Value::as_u64)?;
    if code != 110_000 {
        return None;
    }
    let secs = v
        .get("retry_after")
        .and_then(Value::as_f64)
        .unwrap_or(1.0)
        .clamp(0.2, 5.0);
    Some(Duration::from_secs_f64(secs))
}

/// Parses a search response: `messages` is an array of context groups, the
/// hit has `hit: true`.
pub fn parse_search(v: Value, guild: Option<GuildId>) -> Result<SearchPage> {
    let total = v.get("total_results").and_then(Value::as_u64).unwrap_or(0) as u32;
    let groups = v
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut messages = Vec::new();
    for group in groups {
        let Some(items) = group.as_array() else {
            continue;
        };
        let hit = items
            .iter()
            .find(|m| m.get("hit").and_then(Value::as_bool) == Some(true))
            .or_else(|| items.first());
        let Some(hit) = hit else { continue };
        // A malformed hit is skipped, not fatal.
        if let Ok(w) = serde_json::from_value::<WireMessage>(hit.clone()) {
            let mut m = w.into_domain();
            m.guild_id = m.guild_id.or(guild);
            messages.push(m);
        }
    }
    Ok(SearchPage { total, messages })
}

/// A file picked for upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadFile {
    pub path: std::path::PathBuf,
    /// File name sent to Discord.
    pub name: String,
    pub size: u64,
}

/// Upload limits (bytes). Tier numbers follow Discord's `premium_type`
/// (user) and `premium_tier` (server boost level).
pub const UPLOAD_LIMIT_DEFAULT: u64 = 10 * 1024 * 1024;
pub const MAX_FILES_PER_MESSAGE: usize = 10;

/// Largest file the account may upload in `guild`.
pub fn upload_limit(premium_type: u8, guild_tier: u8) -> u64 {
    const MIB: u64 = 1024 * 1024;
    let user = match premium_type {
        2 => 500 * MIB,
        1 | 3 => 50 * MIB,
        _ => UPLOAD_LIMIT_DEFAULT,
    };
    let guild = match guild_tier {
        2 => 50 * MIB,
        3 => 100 * MIB,
        _ => UPLOAD_LIMIT_DEFAULT,
    };
    user.max(guild)
}

/// Why a set of files cannot be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadError {
    TooMany,
    TooLarge { name: String, limit: u64 },
    Empty { name: String },
}

/// Checks the count and size limits.
pub fn check_upload(files: &[UploadFile], limit: u64) -> std::result::Result<(), UploadError> {
    if files.len() > MAX_FILES_PER_MESSAGE {
        return Err(UploadError::TooMany);
    }
    for f in files {
        if f.size == 0 {
            return Err(UploadError::Empty {
                name: f.name.clone(),
            });
        }
        if f.size > limit {
            return Err(UploadError::TooLarge {
                name: f.name.clone(),
                limit,
            });
        }
    }
    Ok(())
}

pub fn multipart_boundary() -> String {
    format!(
        "ferrocord-{:016x}{:016x}",
        fastrand::u64(..),
        fastrand::u64(..)
    )
}

/// Strips characters that would break out of a quoted header value.
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '\\' | '/'))
        .collect();
    if cleaned.trim().is_empty() {
        "file".into()
    } else {
        cleaned
    }
}

/// Byte layout of the multipart body: `parts[i]` is the preamble written
/// before file `i`; the stream is `head`, then for each file its preamble
/// and contents, then `tail`.
#[derive(Debug, Clone)]
pub struct MultipartLayout {
    pub head: Vec<u8>,
    pub file_heads: Vec<Vec<u8>>,
    pub tail: Vec<u8>,
    pub total_len: u64,
}

impl MultipartLayout {
    pub fn new(boundary: &str, payload_json: &str, files: &[UploadFile]) -> Self {
        let head = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"payload_json\"\r\nContent-Type: application/json\r\n\r\n{payload_json}\r\n"
        )
        .into_bytes();
        let mut file_heads = Vec::with_capacity(files.len());
        for (i, f) in files.iter().enumerate() {
            file_heads.push(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"files[{i}]\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
                    sanitize_filename(&f.name),
                    mime_for(&f.name),
                )
                .into_bytes(),
            );
        }
        let tail = format!("\r\n--{boundary}--\r\n").into_bytes();
        // Every file part ends with CRLF before the next boundary.
        let total_len = head.len() as u64
            + file_heads.iter().map(|h| h.len() as u64).sum::<u64>()
            + files.iter().map(|f| f.size).sum::<u64>()
            + 2 * files.len().saturating_sub(1) as u64
            + tail.len() as u64;
        Self {
            head,
            file_heads,
            tail,
            total_len,
        }
    }
}

/// Content type by extension (Discord sniffs anyway; this only has to be
/// plausible).
pub fn mime_for(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "txt" | "log" => "text/plain",
        "json" => "application/json",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

const CHUNK: usize = 64 * 1024;

/// Streams the multipart body, reading files lazily in 64 KiB chunks.
fn multipart_body(
    layout: &MultipartLayout,
    files: &[UploadFile],
    progress: tokio::sync::mpsc::UnboundedSender<u64>,
) -> reqwest::Body {
    use futures::stream::{self, StreamExt};
    use tokio::io::AsyncReadExt;

    enum Step {
        Bytes(Vec<u8>),
        File(usize),
    }
    let mut steps = vec![Step::Bytes(layout.head.clone())];
    for (i, h) in layout.file_heads.iter().enumerate() {
        if i > 0 {
            steps.push(Step::Bytes(b"\r\n".to_vec()));
        }
        steps.push(Step::Bytes(h.clone()));
        steps.push(Step::File(i));
    }
    steps.push(Step::Bytes(layout.tail.clone()));
    let paths: Vec<std::path::PathBuf> = files.iter().map(|f| f.path.clone()).collect();
    let total = layout.total_len;
    let sent = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let report = move |n: usize| {
        use std::sync::atomic::Ordering;
        let now = sent.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
        // Throttle to ~1% steps so the UI is not flooded.
        let step = (total / 100).max(1);
        if now / step != (now - n as u64) / step || now >= total {
            let _ = progress.send(now.min(total));
        }
    };
    let s = stream::iter(steps).flat_map(move |step| match step {
        Step::Bytes(b) => {
            report(b.len());
            stream::once(async move { Ok::<_, std::io::Error>(b) }).boxed()
        }
        Step::File(i) => {
            let path = paths[i].clone();
            let report = report.clone();
            stream::unfold(
                (None::<tokio::fs::File>, path, false),
                move |(file, path, done)| {
                    let report = report.clone();
                    async move {
                        if done {
                            return None;
                        }
                        let mut file = match file {
                            Some(f) => f,
                            None => match tokio::fs::File::open(&path).await {
                                Ok(f) => f,
                                Err(e) => return Some((Err(e), (None, path, true))),
                            },
                        };
                        let mut buf = vec![0u8; CHUNK];
                        match file.read(&mut buf).await {
                            Ok(0) => None,
                            Ok(n) => {
                                buf.truncate(n);
                                report(n);
                                Some((Ok(buf), (Some(file), path, false)))
                            }
                            Err(e) => Some((Err(e), (None, path, true))),
                        }
                    }
                },
            )
            .boxed()
        }
    });
    reqwest::Body::wrap_stream(s)
}
