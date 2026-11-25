//! In-app updater: checks GitHub Releases for a newer version, shows a
//! banner, and installs the release itself when the install method allows
//! it (see `docs/architecture.md`, "Updates").
//!
//! Zero cost until the first check: no client, thread or timer state
//! beyond a pending sleep. Everything that touches the network or the disk
//! runs in a `Task`; the pure parts (version compare, asset selection,
//! install-method detection, schedule, banner state) are unit-tested.

pub mod extract;
pub mod install;
pub mod method;
pub mod net;
pub mod release;
pub mod schedule;
pub mod version;
pub mod view;

use std::fmt;
use std::path::{Path, PathBuf};

pub use method::{InstallMethod, ManualReason, ScriptLayout};
pub use net::{CheckOutcome, Endpoint};
pub use release::{Asset, Platform, Release};
pub use version::Version;

use crate::store::Store;

/// The repository official releases come from.
pub const DEFAULT_REPO: &str = "FerroApps/Ferrocord";
/// Crate / binary name (release asset prefix).
pub const CRATE: &str = "ferrocord";
pub const APP_ID: &str = crate::config::APP_ID;

/// The GitHub repository (`owner/name`) releases come from:
/// `FERRO_UPDATE_REPO` at build time, for forks that publish their own
/// builds with the same asset names, else [`DEFAULT_REPO`]. A malformed
/// value is ignored.
pub fn repo() -> &'static str {
    repo_from(option_env!("FERRO_UPDATE_REPO"))
}

fn repo_from(over: Option<&'static str>) -> &'static str {
    over.map(str::trim)
        .filter(|r| valid_repo(r))
        .unwrap_or(DEFAULT_REPO)
}

/// `owner/name` with GitHub's characters only (nothing that could change
/// the host or path of the URLs built from it).
fn valid_repo(r: &str) -> bool {
    let seg = |s: &str| {
        !s.is_empty()
            && s.len() <= 100
            && s != "."
            && s != ".."
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    matches!(r.split_once('/'), Some((owner, name)) if seg(owner) && seg(name))
}

/// `releases/latest` of [`repo()`].
pub fn api_url() -> String {
    format!("https://api.github.com/repos/{}/releases/latest", repo())
}

/// Fallback release page.
pub fn releases_page() -> String {
    format!("https://github.com/{}/releases", repo())
}

/// SQLite `meta` keys.
pub const KEY_AUTO: &str = "update.auto";
pub const KEY_DISMISSED: &str = "update.dismissed";
pub const KEY_ETAG: &str = "update.etag";
pub const KEY_RELEASE: &str = "update.release";

/// Why an update step failed (user-facing via `Display`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    Network(String),
    Http(u16),
    RateLimited,
    TooLarge,
    /// A URL or redirect outside the allowed GitHub hosts.
    Insecure(String),
    /// The release has no archive for this platform.
    NoAsset,
    /// Checksum missing, unreadable or not matching.
    Checksum,
    BadArchive(String),
    Io(String),
    Cancelled,
    /// This install method does not self-update.
    NotSupported,
}

impl UpdateError {
    pub fn io(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(e) => write!(f, "network error: {e}"),
            Self::Http(s) => write!(f, "GitHub answered HTTP {s}"),
            Self::RateLimited => {
                write!(f, "GitHub is rate-limiting update checks; try again later")
            }
            Self::TooLarge => write!(f, "the download is larger than expected"),
            Self::Insecure(h) => write!(f, "refused a download from an unexpected location ({h})"),
            Self::NoAsset => write!(f, "this release has no build for this system"),
            Self::Checksum => write!(f, "the download did not match its SHA-256 checksum"),
            Self::BadArchive(e) => write!(f, "the release archive is not valid: {e}"),
            Self::Io(e) => write!(f, "could not write the new files: {e}"),
            Self::Cancelled => write!(f, "cancelled"),
            Self::NotSupported => write!(f, "this installation cannot update itself"),
        }
    }
}

/// What the updater is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Checking {
        manual: bool,
    },
    Downloading {
        received: u64,
        total: Option<u64>,
    },
    Verifying,
    Installing,
    /// Installed: "Restart to finish".
    Ready {
        version: Version,
    },
    /// The install failed; the current install is untouched.
    Failed(String),
}

impl Phase {
    pub fn busy(&self) -> bool {
        matches!(
            self,
            Self::Checking { .. } | Self::Downloading { .. } | Self::Verifying | Self::Installing
        )
    }

    pub fn installing(&self) -> bool {
        matches!(
            self,
            Self::Downloading { .. } | Self::Verifying | Self::Installing
        )
    }
}

/// Result of the last check, shown next to "Check for updates".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStatus {
    Never,
    UpToDate,
    Available(Version),
    /// Mock / screenshot mode.
    Off,
    Error(String),
}

/// Install progress, streamed from the install task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallEvent {
    Progress(u64, Option<u64>),
    Verifying,
    Installing,
    Done(Result<Version, UpdateError>),
}

/// Updater messages (`Message::Update`).
#[derive(Debug, Clone)]
pub enum UpdateMsg {
    /// A timed check is due (only the latest timer generation counts).
    Tick(u64),
    /// Settings → "Check for updates".
    CheckNow,
    Checked {
        manual: bool,
        method: InstallMethod,
        exe: Option<PathBuf>,
        result: Result<CheckOutcome, UpdateError>,
    },
    /// Install method detected (Settings opened before any check).
    Detected {
        method: InstallMethod,
        exe: Option<PathBuf>,
    },
    /// [Update] / [Update now].
    Start,
    Install(InstallEvent),
    /// Cancel the download.
    Cancel,
    /// The banner's [x].
    Dismiss,
    /// "Restart to finish" → [Later].
    Later,
    /// [Restart now].
    Restart,
    ToggleAuto,
    OpenReleasePage,
    /// Copies the `cargo install` command.
    CopyCommand,
    /// Start-up cleanup finished.
    CleanedUp,
}

/// What the top banner shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Banner {
    /// A newer version; the buttons depend on the install method.
    Available {
        version: Version,
        self_update: bool,
        cargo: bool,
    },
    Progress(Phase),
    Ready(Version),
    Failed(String),
}

/// Updater state kept in `App`.
#[derive(Debug)]
pub struct Updater {
    pub current: Version,
    /// "Check for updates automatically".
    pub auto: bool,
    /// The updater may use the network (not mock/screenshot/debug).
    pub allowed: bool,
    pub method: Option<InstallMethod>,
    /// `current_exe()` captured before any install (a replaced Linux
    /// binary reads as "(deleted)" afterwards).
    pub exe: Option<PathBuf>,
    pub latest: Option<Release>,
    pub dismissed: Option<Version>,
    pub etag: Option<String>,
    pub phase: Phase,
    pub status: CheckStatus,
    /// "Later" was chosen after an install.
    pub restart_later: bool,
    /// Generation of the pending timed check.
    pub timer: u64,
    pub cancel: Option<iced::task::Handle>,
}

/// What `Updater::load` reads from the store.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Persisted {
    pub auto: bool,
    pub dismissed: Option<Version>,
    pub etag: Option<String>,
    pub release: Option<Release>,
}

impl Persisted {
    pub fn load(store: &Store) -> Self {
        let meta = |k: &str| store.meta(k).ok().flatten();
        let release = meta(KEY_RELEASE).and_then(|s| serde_json::from_str::<Release>(&s).ok());
        Self {
            auto: meta(KEY_AUTO).as_deref() != Some("0"),
            dismissed: meta(KEY_DISMISSED).and_then(|s| Version::parse(&s)),
            // An ETag without the release it describes is useless (a 304
            // would leave nothing to show).
            etag: meta(KEY_ETAG).filter(|_| release.is_some()),
            release,
        }
    }
}

/// Writes one `meta` value (`None` removes it by storing an empty value).
pub fn save_meta(store: &Store, key: &str, value: &str) {
    if let Err(e) = store.set_meta(key, value) {
        tracing::warn!(error = %e, key, "update: could not save state");
    }
}

impl Updater {
    pub fn new(p: Persisted, allowed: bool) -> Self {
        Self {
            current: Version::current(),
            auto: p.auto,
            allowed,
            method: None,
            exe: None,
            latest: p.release,
            dismissed: p.dismissed,
            etag: p.etag,
            phase: Phase::Idle,
            status: if allowed {
                CheckStatus::Never
            } else {
                CheckStatus::Off
            },
            restart_later: false,
            timer: 0,
            cancel: None,
        }
    }

    /// The latest release, when it is newer than this build.
    pub fn available(&self) -> Option<&Release> {
        self.latest
            .as_ref()
            .filter(|r| r.is_newer_than(self.current))
    }

    pub fn release_page(&self) -> String {
        self.latest
            .as_ref()
            .map_or_else(releases_page, Release::page)
    }

    pub fn can_self_update(&self) -> bool {
        self.method
            .as_ref()
            .is_some_and(InstallMethod::can_self_update)
    }

    /// The banner to show, if any.
    pub fn banner(&self) -> Option<Banner> {
        match &self.phase {
            p if p.installing() => return Some(Banner::Progress(p.clone())),
            Phase::Ready { version } if !self.restart_later => {
                return Some(Banner::Ready(*version));
            }
            Phase::Failed(e) => return Some(Banner::Failed(e.clone())),
            _ => {}
        }
        if matches!(self.phase, Phase::Ready { .. }) {
            return None;
        }
        let r = self.available()?;
        if self.dismissed == Some(r.version) {
            return None;
        }
        Some(Banner::Available {
            version: r.version,
            self_update: self.can_self_update(),
            cargo: self.method == Some(InstallMethod::Cargo),
        })
    }

    /// The banner's [x]: a failed install goes back to idle; an offered
    /// version is dismissed until a newer one appears. Returns the
    /// version to persist.
    pub fn dismiss(&mut self) -> Option<Version> {
        if matches!(self.phase, Phase::Failed(_)) {
            self.phase = Phase::Idle;
            return None;
        }
        let v = self.available()?.version;
        self.dismissed = Some(v);
        Some(v)
    }

    /// Applies a check result. Returns whether the cached release/ETag
    /// changed (to persist them).
    pub fn apply_check(&mut self, result: &Result<CheckOutcome, UpdateError>) -> bool {
        if matches!(self.phase, Phase::Checking { .. }) {
            self.phase = Phase::Idle;
        }
        let mut changed = false;
        match result {
            Ok(CheckOutcome::Release { release, etag }) => {
                self.latest = Some(release.clone());
                self.etag = etag.clone();
                changed = true;
            }
            Ok(CheckOutcome::NoRelease) => {
                changed = self.latest.is_some() || self.etag.is_some();
                self.latest = None;
                self.etag = None;
            }
            Ok(CheckOutcome::NotModified) => {}
            Ok(CheckOutcome::RateLimited(_)) => {
                self.status = CheckStatus::Error(UpdateError::RateLimited.to_string());
                return false;
            }
            Err(e) => {
                self.status = CheckStatus::Error(e.to_string());
                return false;
            }
        }
        self.status = match self.available() {
            Some(r) => CheckStatus::Available(r.version),
            None => CheckStatus::UpToDate,
        };
        changed
    }
}

/// Runs a check: detects the install method (blocking file checks, on
/// the blocking pool) and asks GitHub.
pub async fn run_check(
    endpoint: Endpoint,
    etag: Option<String>,
    exe: Option<PathBuf>,
) -> (
    InstallMethod,
    Option<PathBuf>,
    Result<CheckOutcome, UpdateError>,
) {
    let (method, exe) = detect_async(exe).await;
    let result = net::check(&endpoint, etag.as_deref()).await;
    (method, exe, result)
}

/// Detects the install method off the UI thread.
pub async fn detect_async(exe: Option<PathBuf>) -> (InstallMethod, Option<PathBuf>) {
    tokio::task::spawn_blocking(move || {
        let exe = exe.or_else(method::current_exe);
        let m = match &exe {
            Some(e) => method::detect(&method::gather(e)),
            None => InstallMethod::Manual(ManualReason::SystemPath),
        };
        (m, exe)
    })
    .await
    .unwrap_or((InstallMethod::Manual(ManualReason::SystemPath), None))
}

/// The folder an install writes to (temp files go there too, so every
/// rename stays on one filesystem).
pub fn install_dir(method: &InstallMethod) -> Option<&Path> {
    match method {
        InstallMethod::Script(l) => Some(&l.bin_dir),
        InstallMethod::WindowsZip { dir } => Some(dir),
        _ => None,
    }
}

/// Downloads, verifies and installs `release`. `on_event` gets progress.
/// Any failure leaves the install untouched and removes the temp folder.
pub async fn perform_install(
    endpoint: &Endpoint,
    release: &Release,
    method: &InstallMethod,
    platform: Platform,
    mut on_event: impl FnMut(InstallEvent),
) -> Result<Version, UpdateError> {
    let dir = install_dir(method).ok_or(UpdateError::NotSupported)?;
    let (archive, sum) = release.assets_for(platform).ok_or(UpdateError::NoAsset)?;
    if archive.size > net::MAX_DOWNLOAD {
        return Err(UpdateError::TooLarge);
    }
    let tmp = install::TempDir::new_in(dir)?;
    let sum_text = net::fetch_text(endpoint, &sum.url).await?;
    let expected = release::parse_sha256(&sum_text, &archive.name).ok_or(UpdateError::Checksum)?;
    let archive_path = tmp.path().join(&archive.name);
    let mut last = (0u64, u64::MAX);
    let digest = net::download(endpoint, &archive.url, &archive_path, |got, total| {
        // About one event per percent (or per MiB without a length).
        let step = total.map_or(1 << 20, |t| (t / 100).max(64 * 1024));
        if got == 0 || got / step != last.1 || Some(got) == total {
            last = (got, got / step);
            on_event(InstallEvent::Progress(got, total));
        }
    })
    .await?;
    on_event(InstallEvent::Verifying);
    if digest != expected {
        tracing::warn!(asset = %archive.name, "update: checksum mismatch, aborting");
        return Err(UpdateError::Checksum);
    }
    on_event(InstallEvent::Installing);
    let method = method.clone();
    let version = release.version;
    let name = archive.name.clone();
    tokio::task::spawn_blocking(move || {
        let out = tmp.path().join("x");
        std::fs::create_dir(&out).map_err(UpdateError::io)?;
        let archive = tmp.path().join(&name);
        if name.ends_with(".zip") {
            extract::extract_zip(&archive, &out, extract::LIMITS)?;
        } else {
            extract::extract_tar_gz(&archive, &out, extract::LIMITS)?;
        }
        let pkg = extract::package_root(&out)?;
        match &method {
            InstallMethod::Script(layout) => install::install_script(layout, &pkg, version)?,
            InstallMethod::WindowsZip { dir } => install::install_zip(dir, &pkg)?,
            _ => return Err(UpdateError::NotSupported),
        }
        drop(tmp);
        Ok(version)
    })
    .await
    .map_err(|_| UpdateError::Cancelled)?
}

/// The install as a stream of events for `Task::run` (dropping it, via
/// the task's abort handle, cancels the download).
pub fn install_stream(
    endpoint: Endpoint,
    release: Release,
    method: InstallMethod,
) -> impl futures::Stream<Item = InstallEvent> + Send + 'static {
    iced::stream::channel(
        64,
        async move |mut tx: futures::channel::mpsc::Sender<InstallEvent>| {
            let mut progress = tx.clone();
            let r = perform_install(&endpoint, &release, &method, Platform::current(), |ev| {
                // A full channel just drops a progress tick.
                let _ = progress.try_send(ev);
            })
            .await;
            match &r {
                Ok(v) => tracing::info!(version = %v, "update installed"),
                Err(e) => tracing::warn!(error = %e, "update failed"),
            }
            use futures::SinkExt;
            let _ = tx.send(InstallEvent::Done(r)).await;
        },
    )
}

/// Arguments for the restarted process: the same ones, plus
/// `--minimized` when the window is hidden (never twice).
pub fn restart_args(args: &[String], hidden: bool) -> Vec<String> {
    let mut out: Vec<String> = args
        .iter()
        .filter(|a| a.as_str() != "--minimized")
        .cloned()
        .collect();
    if hidden {
        out.push("--minimized".to_owned());
    }
    out
}

/// The binary to start after an install: the path the install wrote.
pub fn restart_target(method: Option<&InstallMethod>, exe: Option<&Path>) -> Option<PathBuf> {
    match method {
        Some(InstallMethod::Script(l)) => Some(l.bin_path(CRATE)),
        Some(InstallMethod::WindowsZip { dir }) => Some(dir.join(install::exe_file(CRATE, true))),
        _ => exe.map(Path::to_path_buf),
    }
}

/// Starts the new version (detached). The caller releases the
/// single-instance lock first and quits right after.
pub fn spawn_restart(target: &Path, args: &[String]) -> std::io::Result<()> {
    std::process::Command::new(target)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str) -> Release {
        let version = Version::parse(tag).unwrap();
        Release {
            tag: tag.into(),
            version,
            html_url: format!("https://github.com/FerroApps/Ferrocord/releases/tag/{tag}"),
            assets: vec![],
        }
    }

    #[test]
    fn update_repo_override_is_validated() {
        assert_eq!(repo_from(None), DEFAULT_REPO);
        assert_eq!(repo_from(Some("Someone/Ferrocord")), "Someone/Ferrocord");
        assert_eq!(repo_from(Some(" a-b_c.d/e.f ")), "a-b_c.d/e.f");
        for bad in [
            "",
            "Ferrocord",
            "a/b/c",
            "../x",
            "a/..",
            "evil.example/x?y",
            "a b/c",
            "a/b#c",
            "@x/y",
        ] {
            assert_eq!(repo_from(Some(bad)), DEFAULT_REPO, "{bad:?}");
        }
        assert!(api_url().starts_with("https://api.github.com/repos/"));
        assert!(releases_page().starts_with("https://github.com/"));
    }

    fn updater() -> Updater {
        let mut u = Updater::new(Persisted::default(), true);
        u.current = Version::parse("0.17.1").unwrap();
        u
    }

    fn found(r: Release) -> Result<CheckOutcome, UpdateError> {
        Ok(CheckOutcome::Release {
            release: r,
            etag: Some("\"e1\"".into()),
        })
    }

    #[test]
    fn a_newer_release_shows_the_banner() {
        let mut u = updater();
        assert_eq!(u.banner(), None);
        assert!(u.apply_check(&found(rel("v0.18.0"))));
        assert_eq!(u.status, CheckStatus::Available(rel("v0.18.0").version));
        assert_eq!(
            u.banner(),
            Some(Banner::Available {
                version: rel("v0.18.0").version,
                self_update: false,
                cargo: false
            })
        );
        u.method = Some(InstallMethod::Cargo);
        assert!(matches!(
            u.banner(),
            Some(Banner::Available {
                cargo: true,
                self_update: false,
                ..
            })
        ));
    }

    #[test]
    fn the_same_or_older_release_is_up_to_date() {
        let mut u = updater();
        u.apply_check(&found(rel("v0.17.1")));
        assert_eq!(u.status, CheckStatus::UpToDate);
        assert_eq!(u.banner(), None);
        u.apply_check(&found(rel("v0.16.0")));
        assert_eq!(u.banner(), None);
    }

    #[test]
    fn dismissing_hides_that_version_until_a_newer_one() {
        let mut u = updater();
        u.apply_check(&found(rel("v0.18.0")));
        assert_eq!(u.dismiss(), Some(rel("v0.18.0").version));
        assert_eq!(u.banner(), None);
        // The same release again (or 304): still hidden.
        u.apply_check(&Ok(CheckOutcome::NotModified));
        assert_eq!(u.banner(), None);
        u.apply_check(&found(rel("v0.18.1")));
        assert!(u.banner().is_some());
    }

    #[test]
    fn dismissal_persists_across_restarts() {
        let store = Store::open(None).unwrap();
        save_meta(&store, KEY_DISMISSED, "0.18.0");
        save_meta(&store, KEY_AUTO, "0");
        let r = rel("v0.18.0");
        save_meta(&store, KEY_RELEASE, &serde_json::to_string(&r).unwrap());
        save_meta(&store, KEY_ETAG, "\"abc\"");
        let p = Persisted::load(&store);
        assert_eq!(p.dismissed, Some(r.version));
        assert!(!p.auto);
        assert_eq!(p.release.as_ref(), Some(&r));
        assert_eq!(p.etag.as_deref(), Some("\"abc\""));
        let mut u = Updater::new(p, true);
        u.current = Version::parse("0.17.1").unwrap();
        assert_eq!(u.banner(), None, "dismissed version stays hidden");
        // Defaults: auto on, nothing dismissed; an ETag without its release
        // is dropped.
        let store = Store::open(None).unwrap();
        save_meta(&store, KEY_ETAG, "\"abc\"");
        let p = Persisted::load(&store);
        assert!(p.auto && p.dismissed.is_none() && p.etag.is_none());
    }

    #[test]
    fn failures_and_rate_limits_keep_the_last_release() {
        let mut u = updater();
        u.apply_check(&found(rel("v0.18.0")));
        assert!(!u.apply_check(&Err(UpdateError::Http(500))));
        assert!(matches!(u.status, CheckStatus::Error(_)));
        assert!(u.available().is_some());
        assert!(!u.apply_check(&Ok(CheckOutcome::RateLimited(None))));
        assert!(u.available().is_some());
        // No release any more (deleted / only a pre-release).
        assert!(u.apply_check(&Ok(CheckOutcome::NoRelease)));
        assert_eq!(u.banner(), None);
    }

    #[test]
    fn progress_ready_and_failure_banners() {
        let mut u = updater();
        u.apply_check(&found(rel("v0.18.0")));
        u.phase = Phase::Downloading {
            received: 1,
            total: Some(2),
        };
        assert!(matches!(u.banner(), Some(Banner::Progress(_))));
        u.phase = Phase::Ready {
            version: rel("v0.18.0").version,
        };
        assert_eq!(u.banner(), Some(Banner::Ready(rel("v0.18.0").version)));
        u.restart_later = true;
        assert_eq!(u.banner(), None);
        u.phase = Phase::Failed("x".into());
        assert_eq!(u.banner(), Some(Banner::Failed("x".into())));
        assert_eq!(u.dismiss(), None);
        assert_eq!(u.phase, Phase::Idle);
    }

    #[test]
    fn restart_keeps_args_and_adds_minimized_when_hidden() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(restart_args(&a(&["--no-tray"]), false), a(&["--no-tray"]));
        assert_eq!(restart_args(&a(&[]), true), a(&["--minimized"]));
        assert_eq!(
            restart_args(&a(&["--minimized", "--no-tray"]), true),
            a(&["--no-tray", "--minimized"])
        );
        assert_eq!(restart_args(&a(&["--minimized"]), false), a(&[]));
    }

    #[test]
    fn restart_targets_the_installed_binary() {
        let l = ScriptLayout {
            bin_dir: "/h/.local/bin".into(),
            data_dir: "/h/.local/share".into(),
        };
        assert_eq!(
            restart_target(
                Some(&InstallMethod::Script(l)),
                Some(Path::new("/x (deleted)"))
            ),
            Some(PathBuf::from("/h/.local/bin/ferrocord"))
        );
        let w = InstallMethod::WindowsZip { dir: "/w".into() };
        assert_eq!(
            restart_target(Some(&w), None),
            Some(PathBuf::from("/w/ferrocord.exe"))
        );
        assert_eq!(
            restart_target(None, Some(Path::new("/e"))),
            Some(PathBuf::from("/e"))
        );
    }
}
