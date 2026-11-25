# CLAUDE.md

## Overview
Ferrocord is an unofficial, lightweight native Discord desktop client in Rust
(edition 2024, tokio, iced 0.14). It speaks the user-account Gateway v10 and
REST API directly (`tokio-tungstenite` + `reqwest`), plus voice channels and
DM/group calls (voice gateway v8, UDP/RTP, DAVE E2EE via `davey`, Opus, `cpal`).
Video (camera / Go Live) is not implemented yet: see `docs/voice.md` §6.

## Commands
- `cargo run -- --mock` — run with mock data (no network, in-memory stores).
- `cargo run -- --mock --screenshot <login|login-captcha|login-web|login-web-failed|guild|dms|embeds|voice|call|incoming-call|gifs|gif-trending|gif-favorites|emoji-locked|stickers-locked|settings|settings-<profiles|privacy|status|appearance|accessibility|voice|chat|notifications|keybinds|language|advanced|updates>|plugins|plugin-prompt|plugin-decorations|update-banner|server-menu|server-settings|server-invite|leave-server|delete-server|channel-menu|create-channel|edit-channel|server-roles|member-menu|member-roles|ban-member> out.png` — render a screen to PNG
  (headless: `ICED_BACKEND=tiny-skia xvfb-run -a -s "-screen 0 1280x800x24" ...`).
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- `cargo build --release`. `ferrocord --version` / `-V` (also `ferrocord-player`, `ferrocord-login`).
- Plugins: `examples/plugins/build.sh` (needs `rustup target add wasm32-unknown-unknown`) builds the
  example plugins that `tests/plugins_examples.rs` loads (skipped without them unless
  `FERROCORD_EXPECT_PLUGINS=1`); `cargo test -p ferrocord-plugin-sdk` for the SDK.
- Shared machine: `export CARGO_BUILD_JOBS=2`.

## Architecture
See `docs/architecture.md` (shared Ferro* skeleton) and `docs/protocol.md`
(gateway, REST, rate limits, QR login, UI reference), `docs/voice.md` (voice). Plugins: `docs/plugins.md`. Layout: `src/app`
(state + pure reducers), `src/ui` (views + theme), `src/net` (gateway, REST,
remote auth), `src/store` (SQLite cache), `src/auth` (login + keyring).

## Conventions
- English everywhere; Conventional Commits (`feat:`, `fix:`, `chore:`, `docs:`, `test:`).
- No `unwrap()`/`expect()` outside tests (crate-level deny).
- Only `auth/secrets.rs` touches the `keyring` crate; tokens never hit disk or logs.
- Log via `tracing`; wrap user content in `error::Redacted`; types holding
  personal data implement `Debug` manually.
- Views never hard-code colours/sizes: use `ui::theme`.
- Every new crate needs a one-line justification in the PR description.

## Workflow quirks
- Work on `feat/*` branches off `development`; PRs target `development` only.
- Never push to `main`/`development` directly, never merge.
- `version.yml` (helper `.github/scripts/version.sh`) bumps `Cargo.toml` (do not bump
  by hand) and MERGES `main` into `development` after each main bump (real merge,
  no sync commit); protected branches need a bot bypass.

## Deployment
Releases are automatic: `.github/workflows/release.yml` chains off the "Version" workflow
via `workflow_run` (GITHUB_TOKEN bot pushes don't trigger `push` workflows), builds on
ubuntu-22.04, and creates GitHub Release `v<Cargo version>` with `ferrocord-v<ver>-linux-x86_64.tar.gz`
+ `.sha256` (skips if the release exists; manual re-run: workflow_dispatch). Users run
`install.sh` (`curl ... | sh`; `-s -- --uninstall`); test it with `FERRO_RELEASE_API=file://...`.
CI (`.github/workflows/ci.yml`) runs fmt, clippy and tests on ubuntu-latest.

## Gotchas
- Third-party clients violate Discord's ToS; keep the README warning intact.
- Keep iced default features on (they carry the x11/wayland backends).
- iced comes from the `ferroiced` fork (`[patch.crates-io]` in `Cargo.toml`):
  iced 0.14.1 with tiny-skia redraw optimizations, and `wgpu` and
  `linux-theme-detection` off by default (tiny-skia is the only renderer).
  Patch every iced crate to the same source; after changing it run
  `cargo update -p iced`, then check `Cargo.lock` only changed iced packages.
- Integration tests use local mock WebSocket/HTTP servers; never real accounts.
- Endpoints can be overridden with `FERROCORD_GATEWAY_URL`, `FERROCORD_API_URL`,
  `FERROCORD_REMOTE_AUTH_URL`, `FERROCORD_CDN_URL`.
- Mouse position / Shift live in `app/input.rs` atomics (no per-event messages).
- File dialog: `rfd` (xdg-portal). Clipboard files/images: `arboard`.
- Keyring tests must use `MemoryStore` (no Secret Service in CI).
- No wgpu: it costs 100-200 MB RSS. Re-enable with iced's `wgpu` feature plus
  `ICED_BACKEND=wgpu` if ever needed.
- Login captcha: Discord's page opens in the `ferrocord-login` helper (`wry`; WebView2 / WebKitGTK 4.1,
  build needs `libwebkit2gtk-4.1-dev`). Only the helper may link the webview (CI ldd check). Never render
  hCaptcha on our own page or fake an origin; never solve captchas. REST identity headers live in `net/identity.rs`.
- Voice: static libopus (needs `cmake`), `cpal` (`libasound2-dev`); the voice subscription exists
  only in a call. DAVE is mandatory (close 4017); keep `davey` calls under `catch_unwind`.
- Unicode emoji are Twemoji (`assets/fonts/Twemoji.ttf`, COLRv0, CC-BY 4.0, family "Ferrocord Twemoji"): use the emoji span (`ui::emoji_font::emoji_text`/`emoji_runs`/`emoji_font()`). `emoji_font::install()` (in `main`) loads it zero-copy and puts it first in cosmic-text's common fallback, so the composer (`text_editor`, no spans) gets Twemoji too. swash draws COLRv0 and CBDT/sbix in colour, never COLRv1: do not swap in a COLRv1 or SVG-only font. No system emoji font is used.
- Custom emoji / stickers without Nitro: `app/nitro.rs` decides (`premium_type`, the item's guild vs the open one, `available`, emoji `roles`, sticker format). Locked items stay visible with a lock; picking one shows a footer with an explicit "Send as image" (scaled PNG / animated GIF upload). Never send a locked item silently or as `<:name:id>`.
- User Settings (`app/settings.rs`, `ui/settings.rs`, `docs/protocol.md` §14): account settings go through
  `settings-proto/1` (`net/user_settings.rs`): PATCH only the touched top-level fields, each complete
  (sent top-level fields REPLACE the stored ones), read-modify-write keeping unknown bytes, debounced and
  rolled back on failure. Local ones live in `meta("pref.settings")`. Never add a switch the app does not honour.
- `text_editor` `min_height`/`max_height` limit the text area, then the padding is added: pass `composer::text_height_limits`, never a total height (the 68 px composer bug).
- Animated avatars/decorations: stills in lists, chat, DM list and user panel, playing only while
  their own row is hovered (`App::list_phase`, `anim::Hover` + `Zone`), like the official client;
  only the profile popout plays them unhovered. Never make list cosmetics play unhovered: each
  costs 60 window redraws/s.
- Permissions (`app/permissions.rs`): owner/Administrator -> all; else `@everyone` + roles, then channel
  overwrites (`@everyone`, roles, member). Gate UI actions with `ChatState::channel_permissions` /
  `can_in_guild`, never with "is owner" alone; channels without View Channel are hidden. Server admin
  lives in `app/guild_admin.rs` (`Ui::Admin`), channels in `app/channel_admin.rs` (`Ui::Channel`), roles + member moderation in `app/role_admin.rs`
  (`Ui::Roles`; hierarchy via `permissions::top_position`/`outranks`), REST in
  `net/rest/guild.rs` (docs/protocol.md §15).
- Composer is a `text_editor` (`app/composer.rs`): Enter sends, Shift+Enter newline; cursor columns are bytes. Container `shadow` is safe with ferroiced (tiny-skia clips it to the damage region), but
  the house style keeps the gradient header and sets no shadow (`ui/theme.rs` guard test).
- Video helper (`ferrocord-player`): MP4/MOV is read by our own `mp4.rs` (progressive + fragmented `moof`, QuickTime);
  never go back to the `mp4` crate (its fragment offsets are wrong, it rejects QuickTime sound descriptions, panics on
  hostile tables). OpenH264 only decodes 8-bit 4:2:0 progressive H.264 and needs an IDR to start: `h264.rs` reads the
  SPS so High 10 / 4:2:2 / 4:4:4 / interlaced / HEVC get a named "cannot decode" message, never "no picture could be decoded".
- Platform decoders (`ferrocord-player/platform/`): H.265, AV1, the H.264 OpenH264 refuses and AAC in MKV go to the
  system FFmpeg (Linux: `libloading` dlopen of `libavcodec.so.62..58`, never linked; tiny FFI, structs only through the
  stable `AVPacket`/`AVFrame` prefix, formats by name) or Media Foundation MFTs (Windows: `mfplat.dll` loaded at run
  time, never imported; CI checks with dumpbin). OpenH264/VP8/VP9 stay the default; the platform decoder takes over at
  run time when they fail. `ferrocord-player --backends` lists what the system has; `FERROCORD_PLATFORM_DECODERS=0`
  turns them off, `FERROCORD_VIDEO_DECODER=platform` prefers them. Never link `ffmpeg-sys` or bundle a decoder.
- Plugins (`src/plugins/`, `app/plugins.rs`, `ui/plugins.rs`, SDK in `crates/ferrocord-plugin-sdk`): wasmi with
  fuel + 16 MiB memory per plugin, one worker thread, MessagePack ABI v1 (structs as named maps; additive changes only).
  Zero cost with none enabled: no engine, thread or subscription (`tests/plugins_zero_cost.rs`). Never give plugins raw
  gateway/REST access, a token, files or timers; account actions (`send_message`, reactions) need a user-action grant.
  wasmi needs its `auto-dispatch` feature (else debug builds overflow the stack on long wasm loops). Plugins never draw:
  new extension points must stay declarative and host-rendered.
- Animations never tick the app: draw them with `ui::anim_image` (self-playing, redraws only when its own
  next frame is due, on a shared 60 Hz grid, only while visible) with `App::anim_clock(phase)`; effect layers
  pass a `Timeline`. Size `anim::Class::limits` to the drawn size so typical assets keep every frame (frame
  caps decimate = lower fps). `FERROCORD_FRAME_STATS=1` logs redraws/views/lateness; under Xvfb run
  `xdotool windowfocus` first (unfocused windows pause animations).
- GIF picker media go through `gif_anim::picker_bytes` (100 MB disk LRU in the cache dir, 4 downloads at a
  time); tile animations live in `App::gif_anims` (near-view tiles only, freed when the picker closes).
- Quit (`App::quit`, `src/shutdown.rs`) must leave in < 1 s: iced drops its tokio runtime at exit and that
  WAITS FOR EVERY `spawn_blocking` TASK. Never run an unbounded wait there (notification click watches use
  detached threads, `app/notify.rs`); shutdown work is bounded (`FLUSH_BUDGET`) and a watchdog exits at
  `EXIT_DEADLINE`. Test: `dbus-run-session -- xvfb-run -a cargo test --test quit_deadline`.
- In-app updater (`src/update/`, docs/architecture.md §12): GitHub `FerroApps/Ferrocord` releases only; never in
  `--mock`/`--screenshot`/debug builds (`FERRO_UPDATE_FORCE=1` lifts the debug rule). Self-update only for the
  `install.sh` layout and a writable Windows zip folder; cargo installs get the `cargo install` command, everything
  else the release page. Reuse `install.sh`'s paths and file list (`update::install::SHIPPED`); stage next to the
  target and rename, roll back on failure. Restart runs the installed path, never `current_exe()` ("(deleted)").
