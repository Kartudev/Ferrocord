# Ferrocord

A lightweight, native Discord desktop client written in Rust with
[ferroiced](https://github.com/Kartudev/ferroiced), a dedicated and optimized fork of 
[iced](https://iced.rs). It talks to Discord's Gateway (WebSocket) and REST
API directly and aims to look and behave like the official client while
using a fraction of its memory.

> **Unofficial client.** Ferrocord is not affiliated with, endorsed by or
> supported by Discord Inc. "Discord" is a trademark of Discord Inc. No
> Discord logos, icons or fonts are included; the UI is recreated with Inter
> (SIL OFL 1.1) and Lucide icons (ISC).

> [!WARNING]
> **Using a third-party client breaks Discord's Terms of Service and can get
> your account banned.** Logging in with your account token through Ferrocord
> is "self-botting" as far as Discord is concerned. Use it at your own risk,
> preferably with an account you can afford to lose.

## Install / update

Linux x86_64, no root needed. One command installs and updates (it downloads
the latest [release](https://github.com/FerroApps/Ferrocord/releases/latest), checks
its SHA-256 and puts the binary, menu entry and icon under `~/.local`):

```sh
curl -fsSL https://raw.githubusercontent.com/FerroApps/Ferrocord/main/install.sh | sh
```

Uninstall (removes only what the installer added, never your account data):

```sh
curl -fsSL https://raw.githubusercontent.com/FerroApps/Ferrocord/main/install.sh | sh -s -- --uninstall
```

Manual alternative: download `ferrocord-vX.Y.Z-linux-x86_64.tar.gz` and its
`.sha256` from the Releases page, then:

```sh
sha256sum -c ferrocord-*-linux-x86_64.tar.gz.sha256
tar xzf ferrocord-*-linux-x86_64.tar.gz
./ferrocord-*-linux-x86_64/ferrocord
```

The tarball also has `packaging/dev.ferro.ferrocord.desktop` and `.svg` if you want
a menu entry. Releases are built on Ubuntu 22.04 (glibc 2.35).

### Windows

Windows x86_64: download `ferrocord-vX.Y.Z-windows-x86_64.zip` and its
`.sha256` from the Releases page, check the hash and unzip anywhere:

```powershell
(Get-FileHash ferrocord-*-windows-x86_64.zip).Hash.ToLower()   # compare with the .sha256
Expand-Archive ferrocord-*-windows-x86_64.zip .
.errocord-*-windows-x86_64errocord.exe
```

The zip has `ferrocord.exe` and `ferrocord-login.exe` (Discord's login page in
WebView2); keep them together. The token is stored in Windows Credential
Manager. The Windows build has no
tray icon, single-instance lock or autostart yet, and notification clicks do
not open the channel. To build it yourself install Rust (MSVC toolchain) and
run `cargo build --release`.

## Features

- QR-code login (remote auth) using the official mobile app; "Log in with
  Discord's page" (discord.com/login in a system webview window, where Discord
  shows its own captcha, 2FA and QR prompts; opens on its own when Discord
  asks for a captcha); token login. The session is stored only in the OS
  keyring.
- Multiple accounts: switch between up to five saved accounts from the status
  menu (click your avatar in the user panel) or the login screen, without
  restarting; each account's token has its own keyring entry.
- Server rail with unread pills and mention badges, channel list with
  categories, direct messages and group DMs.
- Message history with Discord markdown (bold, italic, underline, strike,
  code, code blocks, spoilers, quotes, headings, lists, links, mentions,
  custom emoji, timestamps), embeds, image and file attachments, GIF and
  image links shown as bare media, stickers.
- GIF picker (Tenor, via Discord; tiles animate, saved GIFs are cached on
  disk) and sticker picker in the composer; a
  participants/profile panel in DMs.
- Unicode emoji drawn with Twemoji, like the official client. The emoji and
  sticker pickers show every server's items; without Nitro the ones you
  cannot use in the current channel are locked, and picking one offers
  "Send as image" (a small scaled copy uploaded as a normal attachment;
  animated ones stay animated) instead of the Nitro upsell.
- Send messages, typing indicators, unread and mention state (synced with
  read states / `MESSAGE_ACK`), desktop notifications for mentions and DMs.
- Local SQLite cache so the window opens instantly with your last state.
- Gateway: heartbeat with jitter, zombie detection, resume, reconnect with
  exponential backoff, `zlib-stream` compression. REST: per-bucket and global
  rate limiting with 429 retry.
- Voice channels and DM / group calls: join and leave, users and speaking
  indicators, mute and deafen, device selection, input sensitivity,
  per-user volume, incoming call overlay with notification. Calls are
  end-to-end encrypted with Discord's DAVE protocol. Video calls (camera, Go
  Live) are not supported yet.
- Videos play in the chat (MP4/MOV with H.264 and AAC, WebM/MKV with VP8/VP9/
  H.264 and Opus built in; H.265/HEVC, AV1, H.264 High 10 / 4:2:2 / 4:4:4 /
  interlaced and AAC in MKV through the system's decoders): inline player with
  poster, play/pause, seek bar, volume and full screen. Decoding runs in the
  `ferrocord-player` helper, which links no system codec library. For the
  second group it uses what the system provides, like a browser: the FFmpeg
  libraries on Linux (`libavcodec`, opened at run time if installed) and
  Media Foundation on Windows (H.265 needs "HEVC Video Extensions from Device
  Manufacturer", AV1 "AV1 Video Extension" from the Microsoft Store). Without
  them the player says what to install, or offers "Open externally".
- Plugins: community add-ons as small sandboxed WebAssembly modules (no
  JavaScript, CSS or webview). They can rewrite what you send, add `/commands`
  and message menu items, and put badges or notes under messages; they run on
  their own thread with time and memory limits and only get the permissions
  you grant. Off by default and free when unused. See
  [Plugins](#plugins) and [`docs/plugins.md`](docs/plugins.md).

## Build and run

Requirements: Rust stable (1.88+), and on Linux `pkg-config`, `cmake` and a
C/C++ compiler (the H.264 decoder is built from source), `libssl-dev`,
`libdbus-1-dev`, `libxkbcommon-dev`, `libwayland-dev`, `libasound2-dev`,
`libwebkit2gtk-4.1-dev` (login helper only).
`cargo build` builds `ferrocord`, `ferrocord-player` and `ferrocord-login`
(`.exe` on Windows); the helpers must sit next to the app
(`--no-default-features` builds neither). Without `ferrocord-player`, videos
and Tenor/Giphy GIFs (which are MP4) cannot play.
`ferrocord-login` shows Discord's login page in the system webview: WebView2 on
Windows (built into Windows 11), WebKitGTK 4.1 on Linux (`libwebkit2gtk-4.1-0`).
Only the helper links the browser engine; it runs only while you log in.

```sh
cargo build --release
./target/release/ferrocord            # real client
./target/release/ferrocord --mock     # demo data, no network
```

A Secret Service provider (KWallet on KDE, GNOME Keyring) must be running;
without one, login is disabled rather than storing the token in a file.

**Local data.** The message cache (`$XDG_DATA_HOME/ferrocord/cache.sqlite3`)
and the log (`$XDG_STATE_HOME/ferrocord/log`) are *not* encrypted: the cache
holds message text, names and channel lists in plain text (never the
token). Both are created owner-only (directories `0700`, files `0600`).
Delete the data directory to wipe the cache; logging out clears it too.

Useful environment variables:

| Variable | Purpose |
|---|---|
| `RUST_LOG` | Log filter (default `info`). Logs never contain tokens or message text. |
| `FERRO_THEME=light` | Light theme (dark is the default). |
| `FERROCORD_NOTIFY_PREVIEW=0` | Notifications show only the sender. |
| `FERROCORD_ZLIB=0` | Disable gateway transport compression. |
| `FERRO_TRACE_STARTUP=1` | Log `first_frame_ms`. |
| `FERROCORD_PLUGINS_DIR` | Plugins folder (also enables plugins in `--mock` mode). |
| `FERROCORD_PLUGIN_LOG=1` | Show plugin log output (it may contain message text). |
| `ICED_BACKEND=wgpu` | Use the GPU renderer (default is the `tiny-skia` CPU renderer, which keeps idle memory around 50 MB). |

Screenshots of every main screen with mock data:

```sh
ICED_BACKEND=tiny-skia xvfb-run -a -s "-screen 0 1280x800x24" \
  cargo run -- --mock --screenshot guild docs/screenshots/guild.png
```

## Plugins

> [!WARNING]
> **Plugins are third-party code.** Ferrocord runs them in a sandbox and
> limits each one to the permissions you grant (shown before you enable it),
> but only install plugins you trust. **Automating your account breaks
> Discord's Terms of Service:** a plugin that sends messages, reacts or
> replies on its own (a "selfbot") can get your account banned. Ferrocord only
> lets plugins send when you explicitly run their command or menu item, and
> never ships such a plugin.

Copy a plugin folder (or a `.ferroplug` file) into the plugins folder
(`~/.config/ferrocord/plugins/` on Linux,
`%APPDATA%\ferro\ferrocord\config\plugins\` on Windows), then open
User Settings → Plugins (in the sidebar), press **Reload plugins** and enable it. There is
no plugin store. Three examples live in [`examples/plugins/`](examples/plugins)
(text replacements and `/shrug`, a word highlighter, "Copy as Quote"); writing
your own is a few lines of Rust with the SDK in
[`crates/ferrocord-plugin-sdk`](crates/ferrocord-plugin-sdk), or any language
that compiles to WebAssembly. Everything else (the ABI, permissions, limits)
is in [`docs/plugins.md`](docs/plugins.md).

With no plugin enabled nothing of the plugin system runs (no engine, no
thread). Each enabled plugin costs about 1-2 MB of memory.

## Development

See [`CLAUDE.md`](CLAUDE.md), [`docs/architecture.md`](docs/architecture.md)
and [`docs/protocol.md`](docs/protocol.md).

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## License

MIT, see [LICENSE](LICENSE). Bundled fonts: Inter (SIL OFL 1.1, see
`assets/fonts/OFL.txt`). Icons: Lucide (ISC, see `assets/icons/LICENSE`).
Emoji: [Twemoji](https://github.com/jdecked/twemoji) by Twitter, jdecked and
contributors, licensed [CC-BY 4.0](https://creativecommons.org/licenses/by/4.0/)
(COLRv0 font build, converted and renamed; see `assets/fonts/Twemoji-LICENSE.txt`).
See [THIRD_PARTY.md](THIRD_PARTY.md).
