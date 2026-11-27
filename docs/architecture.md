# Architecture

> Shared skeleton for the Ferro* family (Ferrogram, Ferrotune, Ferrocord,
> Ferrochat). All four clients start from this document. Sections marked
> **(app-specific)** are filled in per repository; everything else must stay
> identical across the four repos so that a fix or convention in one applies
> to all of them.

## 1. Goals and budgets

| Budget | Target | How we stay inside it |
|---|---|---|
| Idle RAM (logged in) | < 150 MB RSS | Bounded in-memory caches, lazy image decode, SQLite as the source of truth for history, `tiny-skia` CPU renderer (wgpu not built). |
| Cold start | < 1 s to first frame | No network on the critical path: the first frame is rendered from the SQLite cache; the session is restored from the keyring in a background `Task`. |
| Stability | No panics in normal use | `#![deny(clippy::unwrap_used, clippy::expect_used)]` outside tests, typed errors, every network failure is a state, not a crash. |

## 2. Module layout

Every repository has the same top-level layout. Names of files inside each
module may differ, the module boundaries may not.

```
src/
├── main.rs          # Entry point: logging init, config, iced::application(..).run()
├── lib.rs           # Re-exports modules so integration tests can reach them
├── app/             # State + update (the "Elm" side). No I/O here.
│   ├── mod.rs       #   App struct, boot(), update(), subscription()
│   ├── message.rs   #   Message enum (UI events + net events + store events)
│   └── state.rs     #   Pure state types and reducers (unit-tested)
├── ui/              # Views. Pure functions &State -> Element<Message>.
│   ├── mod.rs       #   view() dispatcher by Screen
│   ├── theme.rs     #   Palette, typography, spacing, widget styles
│   ├── icons.rs     #   Lucide SVG icons (ISC licence), embedded with include_bytes!
│   └── <screen>.rs  #   One file per screen (login, sidebar, chat, ...)
├── net/             # Protocol client. Owns sockets, never touches UI types.
│   ├── mod.rs       #   Client handle + Event enum exposed to app/
│   ├── types.rs     #   Wire types (serde) and their conversion to domain types
│   └── ...          #   (app-specific: gateway.rs, rest.rs, mtproto.rs, ...)
├── store/           # Local cache: SQLite via rusqlite.
│   ├── mod.rs       #   Store handle, migrations, typed queries
│   └── schema.sql   #   Embedded with include_str!, versioned by PRAGMA user_version
├── auth/            # Login flows + keyring access.
│   ├── mod.rs       #   AuthState machine (unit-tested)
│   └── secrets.rs   #   The ONLY module allowed to call the `keyring` crate
├── config.rs        # Paths (XDG via `directories`), feature flags, env overrides
├── error.rs         # crate::Error (thiserror) + Result alias
└── mock.rs          # Mock data used by tests and `--mock` screenshot mode
```

Dependency direction (enforced by review, not by the compiler):

```
ui ──▶ app ──▶ net
        │ └──▶ store
        └────▶ auth ──▶ (keyring)
```

`ui` only reads `app` state and emits `Message`s. `net`, `store` and `auth`
never import `ui` or `iced` widgets; they expose plain async APIs and event
streams so they can be tested without a window.

## 3. State model

The app follows iced's Elm architecture.

* **`App`** owns all state. It is a tree of plain structs: `Screen` (what is
  shown), `Session` (who is logged in), domain caches (chats, channels,
  playlists, ...) and transient UI state (composer text, scroll offsets,
  hover, open menus).
* **`Message`** is the only way state changes. Three families:
  * `Ui(..)` — user input (click, type, scroll, key press).
  * `Net(net::Event)` — pushed from the protocol client's event stream
    (new message, presence, typing, playback state, connection state).
  * `Done(..)` — completion of a `Task` (REST response, DB query, keyring read).
* **`update(&mut App, Message) -> Task<Message>`** is the only mutation
  point. Heavy logic lives in `app/state.rs` as pure functions
  (`fn apply_event(state: &mut ChatState, ev: &Event)`) so it is unit-tested
  without iced.
* **`subscription(&App)`** exposes long-lived streams: the network event
  stream (via `Subscription::run_with`), keyboard shortcuts, a 1 s tick for
  relative timestamps (only while visible), and MPRIS / notification
  callbacks where applicable.
* **Connection state** is explicit: `Disconnected → Connecting →
  Connected | Reconnecting { attempt, next_at }`. Reconnects use exponential
  backoff with jitter (1 s → 60 s cap). The UI renders a thin banner while
  not `Connected`, and keeps showing cached data.
* **Caches are bounded.** In-memory message windows hold at most N messages
  per open conversation (default 200); older pages are loaded from SQLite on
  scroll, then from the network. Decoded images live in an LRU keyed by
  media id with a byte budget (default 48 MB).

### Startup sequence

1. `main` initialises `tracing` (with the redaction layer, §6) and config.
2. `App::boot()` opens SQLite synchronously (it is local and fast), loads the
   last-known chat/channel list and the active screen, and returns a `Task`
   batch: restore session from keyring → connect → sync deltas.
3. First frame is drawn from cached data. Target: < 1 s on a cold cache,
   measured by `FERRO_TRACE_STARTUP=1` which logs `first_frame_ms`.

## 4. Theme system

`ui/theme.rs` is the single source of truth for the look. No view hard-codes a
colour, size or font.

```rust
pub struct Palette {
    pub bg_app: Color,        // window background
    pub bg_sidebar: Color,    // list / navigation column
    pub bg_panel: Color,      // main content panel
    pub bg_elevated: Color,   // popovers, menus, modals
    pub bg_hover: Color,
    pub bg_selected: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub accent: Color,        // brand-like accent (recreated, not copied assets)
    pub accent_text: Color,
    pub danger: Color,
    pub success: Color,
    pub border: Color,
    pub bubble_out: Color,    // chat apps only
    pub bubble_in: Color,     // chat apps only
    pub badge: Color,
    pub badge_muted: Color,
}

pub struct Metrics { /* spacing scale, radii, sidebar widths, avatar sizes, font sizes */ }

pub struct FerroTheme { pub palette: Palette, pub metrics: Metrics, pub dark: bool }
```

* A `FerroTheme` is converted into an `iced::Theme::custom(..)` for the base
  widgets, and module-level style functions (`fn chat_row(theme, status)`,
  `fn bubble(theme, outgoing)`, `fn primary_button(..)`) provide the
  per-widget `Style` closures used by views.
* **Dark theme is the default.** A light palette exists where the official
  client has one; selection follows the `FERRO_THEME` env var or config, and
  defaults to dark.
* **Typography:** Inter (SIL OFL 1.1, Regular/Medium/SemiBold/Bold/Italic)
  embedded via `include_bytes!` and loaded zero-copy into iced's font system
  in `main` before `run` (`ui::emoji_font::install`; `iced::font::load` is
  dropped while a daemon has no window); Unicode emoji use the embedded Twemoji
  COLRv0 font (CC-BY 4.0, `ui::emoji_font`), as the official client does. Font sizes follow a small scale defined in `Metrics`.
* **Icons:** Lucide (ISC licence) SVGs embedded in `ui/icons.rs`. Official
  logos, icon sets and fonts of the service are never included.
* **(app-specific)** The palette values, metrics and layout proportions that
  reproduce the official client are documented in `docs/protocol.md` §UI
  reference or in a table at the end of this file.

## 5. Keyring handling

* `auth/secrets.rs` is the only module that calls the `keyring` crate.
  It exposes:

  ```rust
  pub trait SecretStore: Send + Sync {
      fn get(&self, key: &str) -> Result<Option<Secret>>;
      fn set(&self, key: &str, value: &Secret) -> Result<()>;
      fn delete(&self, key: &str) -> Result<()>;
  }
  pub struct KeyringStore;       // OS keyring (Secret Service / Keychain / Credential Manager)
  pub struct MemoryStore(..);    // tests and --mock mode
  ```

* Service name: `dev.ferro.<app>` (e.g. `dev.ferro.ferrogram`), account key:
  a stable non-secret id such as `session` or `user:<numeric id>`.
* `Secret` wraps the value in `secrecy::SecretString` / `SecretBox<Vec<u8>>`:
  it is zeroised on drop and its `Debug` impl prints `[REDACTED]`.
* Large blobs (e.g. an MTProto auth key or Signal identity store) are encrypted
  with a random key that lives in the keyring; only the ciphertext is written
  to the data directory. Plain-text tokens never touch disk.
* Keyring calls are blocking; they run in `tokio::task::spawn_blocking` from a
  `Task`, never on the UI thread.
* If no Secret Service is available (headless CI), the app shows an explicit
  error on the login screen instead of falling back to a file.

## 6. Logging and redaction

* `tracing` + `tracing-subscriber` with `EnvFilter` (`RUST_LOG`, default
  `info`). Logs go to stderr and to `$XDG_STATE_HOME/<app>/log` (rotated,
  max 5 MB).
* **Never log** tokens, auth keys, phone numbers, email addresses, message
  bodies, file names of user media, or full URLs that contain signatures.
* Types that carry such data implement `Debug` manually with redaction
  (`Phone("+34•••••••12")`, `MessageBody(len=42)`).
* A `Redacted<T>` newtype in `error.rs` / `util` is used in `tracing` fields:
  `info!(chat = %id, body = %Redacted(&text))`.
* A unit test greps formatted log output for known secret fixtures to prove
  redaction works.

## 7. Error handling

* `crate::Error` (thiserror) groups errors by origin: `Net`, `Protocol`,
  `Auth`, `Store`, `Keyring`, `Io`, plus `Cancelled`.
* Library-style modules (`net`, `store`, `auth`) return `crate::Result<T>`.
  `anyhow` is not used.
* `update()` never panics: errors coming back from tasks become state
  (`toast`, inline error under a field, connection banner). Each error has a
  user-facing message (`Error::user_message()`) that is free of secrets.
* Auth errors that mean "session revoked" clear the keyring entry and return
  to the login screen.
* Clippy runs with `-D warnings`; `unwrap()`/`expect()` are denied in non-test
  code via crate-level lints.

## 8. Local store (SQLite)

* One database per app at `$XDG_DATA_HOME/<app>/cache.sqlite3`, opened with
  `journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`.
* Migrations are embedded SQL run in order and tracked with `PRAGMA
  user_version`.
* The store is a cache: it can be deleted at any time and is rebuilt from the
  network. It must not contain credentials.
* Access is through a `Store` handle wrapping `rusqlite::Connection` in a
  dedicated blocking thread (or `spawn_blocking` + `Mutex`) so the UI thread
  never waits on disk.

## 9. Notifications and desktop integration

* Desktop notifications via `notify-rust` (D-Bus on Linux). Notification
  text follows the official client's privacy default (sender + preview), and
  can be switched to "sender only".
* Focus-aware: no notification for the conversation that is currently open
  and focused.
* Wayland/KDE: the app sets a stable `app_id` (`dev.ferro.<app>`) so KDE
  groups windows and notifications correctly.

## 10. Testing strategy

* **Unit tests** next to the code: protocol parsing (fixtures under
  `tests/fixtures/`), state reducers, store queries (in-memory SQLite),
  redaction.
* **Integration tests** in `tests/`: run the net client against a local mock
  server (`tokio` TCP/WebSocket or `wiremock`) replaying recorded or
  hand-written responses. No real accounts are used.
* **Screenshot mode:** `cargo run -- --mock --screenshot <screen> <out.png>`
  boots the app with `mock.rs` data and an in-memory secret store, renders the
  requested screen, captures it with `iced::window::screenshot`, writes a PNG
  and exits. Used to produce PR screenshots under Xvfb.

## 11. App-specific sections (app-specific)

* Protocol client design and library choice → `docs/protocol.md`.
* Screens and navigation map.
* Palette table.

### Ferrocord

**Protocol client.** Own Gateway v10 + REST implementation (`net/gateway.rs`,
`net/rest.rs`, `net/ratelimit.rs`, `net/zlib.rs`) and QR login
(`net/remote_auth.rs`); details in `docs/protocol.md`. The gateway and QR
streams run as `Subscription::run_with` keyed by a generation counter (the
token is never hashed); bumping the counter reconnects, dropping the
subscription disconnects.

**Login helper.** "Log in with Discord's page" runs the `ferrocord-login`
helper (`wry` + `tao`: WebView2 / WebKitGTK) as a `Subscription::run_with`
keyed by `web_login_generation`; dropping it kills the helper and wipes its
profile directory. QR captcha → `QrState::CaptchaRequired` + the helper opens
automatically; `auth::WebLoginState` is the pure state machine. See
`docs/protocol.md` §4.1.

**Screens and navigation.**

```
Splash (no cache, restoring) ──▶ Login ──(QR / Discord's page / token)──▶ Main
                                   ▲                       │
                                   └──── logout / 4004 ────┘
Main = Server rail │ Sidebar │ Chat pane
  rail: Home (DMs) · unread DMs · guilds (unread pill, mention badge) · add
  sidebar: guild → categories/channels   |  Home → Friends + DM list
           user panel (avatar, name, log out)
  chat: header (#name, topic, tools) · grouped messages (markdown, embeds,
        attachments, day dividers) · composer · typing indicator
```

`View::Home(Option<ChannelId>)` / `View::Guild { guild, channel }`; the last
channel per guild and the last view are stored in SQLite `meta`.

**Palette.**

| Token | Dark | Light |
|---|---|---|
| `bg_app` (rail) | `#1e1f22` | `#e3e5e8` |
| `bg_sidebar` | `#2b2d31` | `#f2f3f5` |
| `bg_panel` (chat) | `#313338` | `#ffffff` |
| `bg_elevated` | `#111214` | `#ffffff` |
| `bg_hover` | `rgba(78,80,88,.3)` | `rgba(116,127,141,.16)` |
| `bg_selected` | `rgba(78,80,88,.6)` | `rgba(116,127,141,.3)` |
| `text_primary` / header | `#f2f3f5` | `#060607` |
| text normal | `#dbdee1` | `#313338` |
| `text_secondary` | `#b5bac1` | `#4e5058` |
| `text_muted` | `#949ba4` | `#5c5e66` |
| `accent` | `#5865f2` | `#5865f2` |
| `danger` / `badge` | `#f23f43` | `#da373c` |
| `success` | `#23a559` | `#23a559` |
| link | `#00a8fc` | `#006ce7` |

Metrics are in `ui::theme::Metrics::DEFAULT` and `docs/protocol.md` §7.

**Tray and background mode (`src/tray/`, `app/window.rs`, `app/background.rs`).**
The app is an `iced::daemon`. A pure state machine (`app::window::WindowState`)
decides between show / hide / quit: with a StatusNotifierWatcher on the bus,
closing the window CLOSES it (renderer and surfaces are freed, the gateway
stays connected); without one, closing quits. `tray::sni` (Linux, `ksni`)
exports the StatusNotifierItem: left click toggles, menu = Show/Hide, Status
(Online / Idle / Do Not Disturb / Invisible), Mute notifications, Start
minimized, Quit; `NeedsAttention` plus a red-badge icon and the mention count
in the tooltip while `ChatState::attention_count() > 0`. Other platforms use
`tray::stub` (no tray; not implemented). `tray::instance` is a Unix-socket
single-instance lock (a second launch shows the window); `tray::autostart`
writes `~/.config/autostart/dev.ferro.ferrocord.desktop` with `--minimized`.
Preferences live in SQLite `meta` under `pref.*` (kept on logout). Flags:
`--minimized`, `--no-tray`.

**Quitting (`App::quit`, `src/shutdown.rs`).** Tray "Quit", Ctrl+Q and a lost
tray while hidden all end in `App::quit`: the window closes at once, the video
helper is killed, every subscription is dropped (gateway, voice, tray, login
helper with `kill_on_drop`), pending settings get `FLUSH_BUDGET` (350 ms), then
`iced::exit()`. iced then drops its tokio runtime, which waits for every
`spawn_blocking` task without a timeout, so `shutdown::begin` arms a watchdog
that ends the process at `EXIT_DEADLINE` (600 ms) regardless. Never put a wait
of unbounded length on the blocking pool (notification click watches run on
their own detached threads, `app::notify`). `tests/quit_deadline.rs` clicks
Quit in the tray menu of the real binary with a stuck blocking task
(`FERROCORD_MOCK_HANG=1`) and asserts the exit time.

**Plugins (`src/plugins/`, `app/plugins.rs`, `ui/plugins.rs`).** Sandboxed
WebAssembly plugins (wasmi, fuel and memory limits) on one worker thread,
declared by a manifest, reaching the app only through a capability-checked
host ABI; account actions need a user-action grant and go through the app's
own REST paths. Nothing runs while no plugin is enabled. See
`docs/plugins.md`.

## 12. Updates

`src/update/` is the in-app updater (glue in `app/updates.rs`, views in
`update/view.rs`). It is idle until the first check: a check is one small
request, the HTTP client is built per check, and nothing runs in `--mock`,
`--screenshot` or debug builds (`FERRO_UPDATE_FORCE=1` lifts only the debug
rule; mock and screenshot mode never touch the network).

- **Source**: `GET https://api.github.com/repos/FerroApps/Ferrocord/releases/latest`
  with `Accept: application/vnd.github+json`, `User-Agent: ferrocord/<version>`
  and `If-None-Match` (the ETag and the parsed release are cached in the
  `meta` table: `update.etag`, `update.release`). No account data, no
  telemetry, no machine id. 403/429 back off silently (`retry-after` /
  `x-ratelimit-reset`, 1 h to 24 h). `FERRO_RELEASE_API` overrides the URL
  (same meaning as in `install.sh`; HTTPS, or HTTP on loopback for tests).
- **When**: 10–20 s after start (jitter), then every 6 h + up to 30 min,
  also while hidden in the tray (a sleeping `Task`, generation-checked).
  Settings → Updates → "Check for updates automatically" (`update.auto`,
  default on) stops the timed checks; "Check for updates" always works.
- **Compare**: strict `MAJOR.MINOR.PATCH` against `CARGO_PKG_VERSION`
  (`update/version.rs`); drafts, pre-releases and odd tags are ignored.
  Assets are picked by exact name: `ferrocord-v<ver>-linux-x86_64.tar.gz` or
  `ferrocord-v<ver>-windows-x86_64.zip`, each with its `.sha256`.
- **Banner**: a slim bar under the connection banner, in its own fixed
  widget slot (tree shape never changes). "Ferrocord X is available"
  [Update] [Release notes] [x]; [x] stores `update.dismissed` and hides that
  version until a newer one appears. It also shows progress
  (downloading with [Cancel] / verifying / installing), "Restart to finish"
  [Restart now] [Later], and failures with "Open release page".
- **Install method** (`update/method.rs`, pure `detect(&Facts)`):

  | Method | Detected by | Update does |
  |---|---|---|
  | `install.sh` (Linux) | canonical exe = `${XDG_BIN_HOME:-~/.local/bin}/ferrocord` and `${XDG_DATA_HOME:-~/.local/share}/ferrocord/installed-version` exists, bin dir writable | self-update |
  | Windows zip | Windows exe in a writable folder | self-update (rename dance) |
  | `cargo install` | exe in `${CARGO_HOME:-~/.cargo}/bin` | shows `cargo install --git https://github.com/FerroApps/Ferrocord --locked --force` |
  | anything else | system path, Flatpak/Snap/AppImage env, cargo `target/`, read-only folder, other OS | opens the release page |

- **Install** (`perform_install`): temp folder `.ferrocord-update-<pid>-<rand>`
  *inside* the install folder (same filesystem), download the `.sha256`
  and the archive (HTTPS; redirects only to `github.com`,
  `objects.githubusercontent.com`, `release-assets.githubusercontent.com`;
  300 MB cap; timeouts; signed query strings never logged), verify the
  SHA-256, extract safely (`update/extract.rs`: regular files and folders
  only; absolute paths, `..`, symlinks, hard links and devices rejected;
  600 MB / 2000 entries cap), then `update/install.rs`:
  - `install.sh` layout: the same files `install.sh` writes (`ferrocord`,
    `ferrocord-player`, `ferrocord-login`, the desktop entry with the
    rewritten `Exec=`, the icon, `installed-version`), each staged as
    `.<name>.new.<pid>` and renamed over its target, then the desktop caches
    are refreshed like the script does.
  - Windows: each running `.exe` is renamed to `<name>.old` (or
    `<name>.<n>.old` while an older one is still locked) and the new one
    moved in; `.old` files and stale temp folders are deleted at the next
    start (`install::cleanup`, off the UI thread).
  - All switches or none: a failure rolls the done ones back from the old
    copies; the temp folder is always removed. The download runs in an
    abortable task, so [Cancel] (or quitting) drops it.
- **Restart**: runs the installed path (not `current_exe()`, which reads
  "(deleted)" after an update on Linux) with the same arguments, plus
  `--minimized` when the window is hidden, after `tray::instance::release()`
  removed the single-instance socket; then quits through `Ui::Quit`.
  [Later]: the new version runs at the next start.
- **Integrity**: the SHA-256 comes from the same release, so it proves the
  file arrived intact, not who built it. Releases are not signed yet;
  signing them (minisign or cosign) needs a CI secret the maintainer must
  add, after which the updater can verify the signature before installing.
