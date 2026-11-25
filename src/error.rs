//! Crate-wide error type, `Result` alias and the `Redacted` logging helper.

use std::fmt;

/// Every fallible operation in `net`, `store` and `auth` returns this error.
///
/// Variants group errors by origin. None of the payloads may contain secrets
/// (tokens, message bodies): they are built from status codes, close codes
/// and static descriptions only.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Transport-level failure (DNS, TLS, socket, HTTP client).
    #[error("network error: {0}")]
    Net(String),
    /// The server sent something we could not understand, or answered with
    /// an unexpected status.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// HTTP error status from the REST API.
    #[error("HTTP {status}: {message}")]
    Http { status: u16, message: String },
    /// Authentication failed or the session was revoked.
    #[error("authentication failed: {0}")]
    Auth(String),
    /// SQLite cache failure.
    #[error("local cache error: {0}")]
    Store(String),
    /// OS keyring failure (including "no Secret Service available").
    #[error("keyring error: {0}")]
    Keyring(String),
    /// Local file system failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Discord asked for a captcha before allowing the action. Ferrocord
    /// never solves captchas itself; the login flow lets the user solve it
    /// on Discord's own page.
    #[error("captcha required")]
    CaptchaRequired(Box<crate::net::captcha::CaptchaChallenge>),
    /// Discord wants a fresh two-factor check (HTTP 401, code 60003) before
    /// the action; the ticket answers it (`net::mfa`). Not a revoked session.
    #[error("two-factor authentication required")]
    MfaRequired(Box<crate::net::mfa::MfaChallenge>),
    /// The operation was cancelled (task dropped, logout, shutdown).
    #[error("cancelled")]
    Cancelled,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    /// A short message suitable for the UI. Never contains secrets.
    pub fn user_message(&self) -> String {
        match self {
            Error::Net(_) => "Could not reach Discord. Check your connection.".into(),
            Error::Protocol(_) => "Discord sent an unexpected response.".into(),
            Error::Http { status: 401, .. } => "Your session is no longer valid.".into(),
            Error::Http { status: 403, .. } => "You do not have permission to do that.".into(),
            Error::Http { status: 429, .. } => "You are being rate limited.".into(),
            Error::Http { status, .. } => format!("Request failed (HTTP {status})."),
            Error::Auth(reason) => reason.clone(),
            Error::Store(_) => "The local cache could not be read.".into(),
            Error::Keyring(reason) => format!("Secure storage unavailable: {reason}"),
            Error::Io(_) => "A local file could not be accessed.".into(),
            Error::MfaRequired(_) => {
                "Discord asks for your two-factor authentication code for this.".into()
            }
            Error::CaptchaRequired(_) => "Discord asked for a captcha before allowing this. \
                Open it on discord.com (or in the official app) once, then try again here."
                .into(),
            Error::Cancelled => "Cancelled.".into(),
        }
    }

    /// Whether this error means the stored session is revoked and must be
    /// cleared from the keyring.
    pub fn is_session_revoked(&self) -> bool {
        matches!(self, Error::Http { status: 401, .. })
            || matches!(self, Error::Auth(r) if r.contains("revoked") || r.contains("invalid token"))
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Store(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        // serde_json errors only carry line/column + a description of the
        // expected type, never the payload itself.
        Error::Protocol(e.to_string())
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        // `without_url` strips the request URL, which may contain signed
        // attachment parameters.
        Error::Net(e.without_url().to_string())
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Error::Net(e.to_string())
    }
}

/// Wraps a value for use in `tracing` fields so that only its shape (length)
/// is logged, never its content.
///
/// ```
/// use ferrocord::error::Redacted;
/// assert_eq!(format!("{}", Redacted("hello")), "<redacted len=5>");
/// ```
pub struct Redacted<T>(pub T);

impl<T: AsRef<str>> fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<redacted len={}>", self.0.as_ref().len())
    }
}

impl<T: AsRef<str>> fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Removes anything that looks like a Discord token from `input`.
///
/// This is defence in depth for the log writer: secrets are never passed to
/// `tracing` on purpose, but if one slips through (e.g. inside a third-party
/// error string) it is scrubbed before it reaches stderr or the log file.
///
/// A user token has the shape `base64(user_id).base64(timestamp).hmac`, i.e.
/// three runs of URL-safe base64 characters separated by dots, the first one
/// at least 18 characters long and the last one at least 20.
pub fn scrub_secrets(input: &str) -> std::borrow::Cow<'_, str> {
    fn is_tok(c: u8) -> bool {
        c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
    }
    let bytes = input.as_bytes();
    let mut out: Option<String> = None;
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        if !is_tok(bytes[i]) || (i > 0 && is_tok(bytes[i - 1])) {
            i += 1;
            continue;
        }
        // Try to match three dot-separated runs starting at `i`.
        let mut j = i;
        let mut runs = [0usize; 3];
        let mut ok = true;
        for (k, run) in runs.iter_mut().enumerate() {
            let start = j;
            while j < bytes.len() && is_tok(bytes[j]) {
                j += 1;
            }
            *run = j - start;
            if k < 2 {
                if j < bytes.len() && bytes[j] == b'.' {
                    j += 1;
                } else {
                    ok = false;
                    break;
                }
            }
        }
        if ok && runs[0] >= 18 && runs[1] >= 5 && runs[2] >= 20 {
            let s = out.get_or_insert_with(|| String::with_capacity(input.len()));
            s.push_str(&input[last..i]);
            s.push_str("[REDACTED]");
            last = j;
            i = j;
        } else {
            i += 1;
        }
    }
    match out {
        Some(mut s) => {
            s.push_str(&input[last..]);
            std::borrow::Cow::Owned(s)
        }
        None => std::borrow::Cow::Borrowed(input),
    }
}

/// Log sink used by `tracing-subscriber`: scrubs secrets from every
/// formatted line and writes it to stderr and a size-rotated log file.
pub mod log_sink {
    use std::fs::{File, OpenOptions};
    use std::io::{self, Write};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::fmt::MakeWriter;

    /// Rotate the log file when it grows past this size.
    pub const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

    struct RotatingFile {
        path: PathBuf,
        file: File,
        size: u64,
    }

    impl RotatingFile {
        fn open(path: PathBuf) -> io::Result<Self> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
                crate::config::restrict_permissions(dir)?;
            }
            let file = Self::open_file(&path)?;
            let size = file.metadata().map(|m| m.len()).unwrap_or(0);
            Ok(Self { path, file, size })
        }

        /// Owner-only (0600) append-mode log file.
        fn open_file(path: &std::path::Path) -> io::Result<File> {
            let mut o = OpenOptions::new();
            o.create(true).append(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                o.mode(0o600);
            }
            let f = o.open(path)?;
            crate::config::restrict_permissions(path)?;
            Ok(f)
        }

        fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
            if self.size + bytes.len() as u64 > MAX_LOG_BYTES {
                let rotated = self.path.with_extension("1");
                let _ = std::fs::rename(&self.path, rotated);
                self.file = Self::open_file(&self.path)?;
                self.size = 0;
            }
            self.file.write_all(bytes)?;
            self.size += bytes.len() as u64;
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    pub struct LogSink {
        file: Option<Arc<Mutex<RotatingFile>>>,
        capture: Option<Arc<Mutex<Vec<u8>>>>,
        stderr: bool,
    }

    impl LogSink {
        /// stderr plus (if it can be opened) the log file at `path`.
        pub fn new(path: Option<PathBuf>) -> Self {
            let file = path
                .and_then(|p| RotatingFile::open(p).ok())
                .map(|f| Arc::new(Mutex::new(f)));
            Self {
                file,
                capture: None,
                stderr: true,
            }
        }

        /// In-memory sink for tests.
        pub fn capture(buf: Arc<Mutex<Vec<u8>>>) -> Self {
            Self {
                file: None,
                capture: Some(buf),
                stderr: false,
            }
        }
    }

    pub struct LineWriter<'a> {
        sink: &'a LogSink,
        buf: Vec<u8>,
    }

    impl Write for LineWriter<'_> {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.buf.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Drop for LineWriter<'_> {
        fn drop(&mut self) {
            if self.buf.is_empty() {
                return;
            }
            let text = String::from_utf8_lossy(&self.buf);
            let clean = super::scrub_secrets(&text);
            let bytes = clean.as_bytes();
            if self.sink.stderr {
                let _ = io::stderr().write_all(bytes);
            }
            if let Some(f) = &self.sink.file
                && let Ok(mut f) = f.lock()
            {
                let _ = f.write(bytes);
            }
            if let Some(c) = &self.sink.capture
                && let Ok(mut c) = c.lock()
            {
                c.extend_from_slice(bytes);
            }
        }
    }

    impl<'a> MakeWriter<'a> for LogSink {
        type Writer = LineWriter<'a>;
        fn make_writer(&'a self) -> Self::Writer {
            LineWriter {
                sink: self,
                buf: Vec::with_capacity(256),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A token-shaped fixture, assembled at runtime so that no
    /// token-looking literal is committed (secret scanners flag those).
    fn fake_token() -> String {
        [
            "MTIzNDU2Nzg5MDEy".to_string() + "MzQ1Njc4",
            "GaBcDe".into(),
            "abcdefghijklmnop".to_string() + "qrstuvwxyz012345",
        ]
        .join(".")
    }

    #[test]
    fn scrub_removes_token_like_strings() {
        let fake = fake_token();
        let line = format!("error talking to gateway with {fake} attached");
        let s = scrub_secrets(&line);
        assert!(!s.contains(&fake));
        assert_eq!(s, "error talking to gateway with [REDACTED] attached");
    }

    #[test]
    fn scrub_keeps_normal_text() {
        let line = "connected to wss://gateway.discord.gg/?v=10 seq=42 version 1.2.3";
        assert!(matches!(scrub_secrets(line), std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn redacted_only_shows_length() {
        let s = format!("{:?}", Redacted("secret body"));
        assert!(!s.contains("secret"));
        assert!(s.contains("len=11"));
    }

    /// Formats real `tracing` events through the production sink and
    /// greps the output for secret fixtures.
    #[test]
    fn log_output_contains_no_secret_fixtures() {
        use crate::auth::secrets::Secret;
        use std::sync::{Arc, Mutex};
        let buf = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_writer(log_sink::LogSink::capture(buf.clone()))
            .with_ansi(false)
            .finish();
        let body = "my very private message body";
        let fake = fake_token();
        let token = Secret::new(fake.clone());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(body = %Redacted(body), token = ?token, "sending message");
            // Even a careless log line with a raw token is scrubbed.
            tracing::warn!("upstream error: invalid {}", fake);
        });
        let out = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert!(out.contains("sending message"));
        assert!(!out.contains(&fake), "{out}");
        assert!(!out.contains("private message body"), "{out}");
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn user_messages_do_not_leak_details() {
        let fake = fake_token();
        let e = Error::Net(format!("failed with {fake}"));
        assert!(!e.user_message().contains(&fake));
    }
}
