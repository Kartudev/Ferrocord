# Lessons

Things learned while building Ferrocord that are worth remembering.

- **iced 0.14**: keep default features; `default-features = false` breaks the
  `softbuffer`/winit backends. Screenshots under Xvfb work with
  `ICED_BACKEND=tiny-skia` and `window::screenshot`.
- **keyring 4.x** uses the `v1` feature for the classic `Entry` API; missing
  entries are `keyring::Error::NoEntry`. Some error variants carry raw bytes,
  so never `to_string()` them into user-facing text.
- **RSA key generation** (QR login) takes seconds in debug builds; the dev
  profile optimises `rsa`/`num-bigint-dig`, and generation runs in
  `spawn_blocking`.
- **zlib-stream**: one inflate context per connection; frames are only
  complete when they end with `00 00 ff ff`.
- **READY for user accounts** varies with capability flags (guild data under
  `properties`, deduplicated `users`, versioned `read_state`). Parse leniently.
- **Snowflakes** give exact message timestamps (`id >> 22`), so messages need
  no ISO-8601 parsing.
- iced 0.14 `scrollable::direction()` replaces the scrollbar *and its anchor*: call `anchor_bottom()` after `direction()`.
- GitHub push protection rejects token-shaped test fixtures; assemble them at runtime.
- iced `scrollable::scroll_to` offsets of an `anchor_bottom` list are measured from the bottom; a custom `Operation` can read a container's bounds by `Id` to compute them (`app/jump.rs`).
- Mouse position and Shift state are recorded in atomics from the event subscription (returning no message), so reading them in `update` costs no extra view rebuilds.
- `stack` only lets clicks through to a backdrop when the popup layer reports no mouse interaction; wrap popups in `opaque`.
- Reaction echoes of our own actions must be idempotent with the optimistic update.
- iced 0.14 `daemon` drops the compositor when the last window closes, so "hide to tray" = close the window; set `exit_on_close_request: false` and handle `CloseRequested` yourself. `iced::daemon` view closures need a named `fn(&App, window::Id)` (inference fails on lifetimes).
- ksni 0.3 shares zbus 5 with notify-rust/keyring; `spawn()` errors when no StatusNotifierWatcher exists, which is the "no tray host" signal. SNI signals carry no values, so test proxies need `CacheProperties::No`.
- Bot pushes made with `GITHUB_TOKEN` never trigger `push` workflows, so the release workflow chains off the version workflow with `workflow_run` (conclusion `success`, branch `main`, checkout `ref: main` for the bump commit).
- `keyring` 4 opens a *blocking* zbus connection: if any dependency turns on zbus `tokio` (ksni default features, `rfd`/`ashpd` `tokio`) it panics ("Cannot start a runtime from within a runtime") when called on a runtime worker. Keep zbus on `async-io` (`ksni` with `default-features = false, features = ["async-io"]`, `rfd` with `async-std`); `tests/real_keyring.rs` guards it against a real Secret Service.
- **iced diffs widget state by position and type.** Wrapping the body in a `stack`/`column` only while a popup, banner or dialog is open resets every scrollable and hover state under it. Give each optional layer a fixed slot holding an empty `Space` when closed (`ui::view`, test `tree_shape`). A popup backdrop needs `mouse_area(..).interaction(Idle)` so lower layers see a levitating cursor; the menu opens exactly under the pointer and rows below toggled their hover toolbar at its edge.
- `image::Handle::from_rgba` in `view()` gets a new id per frame (re-upload, blinking): cache handles in the app (`App::qr_handle`).
- Under Xvfb with tiny-skia a window repaints only when the next input event arrives, so screenshots taken right after an xdotool action can be one event stale; wiggle the pointer before capturing.
- A button captures press and release, so drag and drop between rail items uses `mouse_area` tiles (press/release/enter/exit) plus a global left-release `DragEnd`.
- Voice gateway v8: heartbeats and RESUME must carry `seq_ack` (last `seq` seen, also from binary frames). Op 25 (DAVE external sender) can arrive before op 4 Session Description; buffer it.
- DAVE is mandatory for voice since March 2026 (close 4017). `davey` (pure Rust, OpenMLS) works offline with a fake external sender: build `ExternalProposal::new_add` from the joiner's key package to test the whole MLS handshake in-process.
- Voice servers send op 5 once per SSRC, not per talk spurt: speaking rings come from received audio activity.
- `cpal::Stream` is not `Send`: keep the streams on their own thread and exchange 20 ms frames through bounded rings.
- **Video helper**: decoding runs in `ferrocord-player` (a separate process), so the app links no codec and a decoder crash or hostile file cannot take the chat down; the helper's only non-glibc library is ALSA's `libasound` (cpal), checked by `.github/scripts/check-helper-libs.sh`. OpenH264 is C++: link libstdc++ statically in CI/release (`CXXSTDLIB=static=stdc++` plus `-L` to `libstdc++.a`).
- `matroska-demuxer`'s `seek` lands on the first frame AFTER a timestamp, not on a keyframe: the helper scans the keyframe timestamps once and seeks to `key - 1`; seeking to 0 reopens the file (browser-recorded WebM has neither cues nor a duration).
- **Platform decoders without a hard dependency**: `dlopen` the system FFmpeg (`libloading`, sonames 62 down to 58 with the matching libavutil) and use only the send/receive API, AVOptions and name lookups. Enum values moved between FFmpeg versions (pixel formats: XvMC removal shifted them), so decoders are found by name and formats read back with `av_get_pix_fmt_name`; the only struct fields touched are the `AVPacket` (`pts`/`dts`/`data`/`size` at 8/16/24/32) and `AVFrame` (`data`/`linesize`/`extended_data`/`width`/`height`/`nb_samples`/`format` up to 116) prefixes, unchanged from FFmpeg 4 to 8. `AVFrame.pts` moved, so pictures take the smallest pending packet time. AAC gets ADTS headers built from the AudioSpecificConfig instead of `extradata`. Set `av_log_set_level(AV_LOG_QUIET)`: the app reports the helper's last stderr line. FFmpeg's own `av1` decoder is hardware-only: use `libdav1d`/`libaom-av1`.
- Windows: the `windows` crate imports each function it wraps, so a direct `MFStartup` call would make the helper fail to start on Windows N (no `mfplat.dll`); load the five `mfplat` entry points with `libloading` and use only COM vtables. Sync software MFTs only (`MFT_ENUM_FLAG_SYNCMFT | LOCALMFT | SORTANDFILTER | UNTRUSTED_STOREMFT` so Store codec extensions are found); a decoder that does not provide samples needs one of `cbSize` per `ProcessOutput`; `MF_E_TRANSFORM_STREAM_CHANGE` means renegotiate the output type. The MS H.264 decoder has no High 10 / 4:4:4.
- `matroska-demuxer`'s `seek` can report success and then the end of the stream (ffmpeg-written MKV with AAC): check that a frame follows, else reopen and read forward.
- With a sound device the audio the device consumed is the playback clock; a stream whose callbacks never run (device opened, not playing) must fall back to the wall clock or the picture freezes.
- Image handles for video frames are created in `update` per frame; the event stream drops stale frames so a slow UI never queues megabytes.
- **text_editor** (multi-line composer): bind Enter via `key_binding` (`Binding::Custom` to send, `Binding::Enter` for Shift+Enter); the stock Up/Enter keys are captured, so the "Up edits last message" shortcut must be a binding too. `text_editor::Cursor` columns are *byte* offsets (a char count panics in cosmic-text and poisons the global font system for every later test). `window::screenshot` replays the last frame, so text set programmatically right before it may not appear: use a real run with xdotool for composer screenshots.
- **Emoji**: the default font has no colour glyphs. Unicode emoji go in their own spans with the bundled Twemoji family (`ui::emoji_font`, grapheme-cluster segmentation).
- **Twemoji in tiny-skia**: swash draws COLRv0 layers in colour (`Source::ColorOutline`, tried first by cosmic-text), so a COLRv0 Twemoji (Mozilla's twemoji-colr build, ~2 MB) renders crisp colour emoji at any size; its `ccmp` ligatures already handle VS16, keycaps, flags, tag flags, skin tones and ZWJ. Debug-mode idle RSS in mock mode: 103 MB vs 124 MB with the bundled 10.8 MB CBDT Noto. Load fonts as `fontdb::Source::Binary(Arc::new(&'static [u8]))`: iced's `.font(bytes)` copies them to the heap. cosmic-text's fallback lists are a `Fallback` trait set when the `FontSystem` is built: rebuild iced's global one (`font_system().write().raw()`, `into_locale_and_db`) to put a bundled family first.
- **Self-playing widgets**: a custom widget can animate without app ticks by handling `window::Event::RedrawRequested(now)` in `update` (store `now` in its tree state, draw from it) and calling `shell.request_redraw_at(next_frame)`; `view()` never runs per frame and tiny-skia's damage tracking repaints only the widget (`ui::anim_image`). Check `layout.bounds().intersects(viewport)` so off-screen widgets stop asking.
- **Captcha on login**: hCaptcha only runs on its sitekey's domains, so Discord's challenge cannot be rendered on a page of our own (file, custom scheme, loopback) without faking the origin. Showing `https://discord.com/login` itself in the system webview (`ferrocord-login`, `wry`) lets Discord run its own captcha; the page's token is read from its `localStorage` by a document-start script (Discord deletes the global later, so keep a reference at document start).
- A helper spawned from a libtest binary (fake helpers in tests) starts its stdout after `test name ... ` without a newline: print a newline first, and have line parsers ignore non-JSON lines.
- `reqwest` needs the `cookies` feature for `cookie_store(true)`; Discord's `__dcfduid`/`__sdcfduid` come back on `GET /experiments`.
- **Font families are looked up by name, first face wins.** A bundled font with the same family as a system font is shadowed by it (system fonts load first). Give bundled fonts their own family name. swash (cosmic-text) draws only bitmap colour glyphs (CBDT/sbix): COLRv1 Noto Color Emoji renders blank or monochrome.
- iced `text_editor` applies `min_height`/`max_height` to the text and adds `padding` afterwards; a total height passed as `min_height` grows the box by the padding a second time.
- Media URLs from Discord's GIF API point at the provider's CDN (several Tenor hosts): fetch them as the browser would (plain client, no Discord headers), not through the Discord-CDN allowlist. Favourite GIFs live in the frecency settings proto (`settings-proto/2`, field 2); writes must be read-modify-write of the whole message.
- **"No picture could be decoded from the H.264 stream"** had several causes, each reproduced with an ffmpeg fixture: a *fragmented* MP4 (browsers, OBS, crash-safe recorders) keeps its samples in `moof`/`trun`, so the empty `moov` sample table gave zero packets (the `mp4` crate also mis-places fragment samples at `base_data_offset`); QuickTime `.mov` failed in the `mp4` crate's sound-description parser; and OpenH264 returns nothing, with no error worth showing, for High 10, 4:2:2, 4:4:4 and interlaced SPSs and for a stream without any IDR. Read the SPS to name the format, and test new container layouts with real files (`tests/fixtures/video`). OpenH264's error concealment cannot start a stream at a recovery-point I-frame cleanly (freeze modes hold output until an IDR; the others drop B-frames).
- symphonia 0.5: `AacDecoder::codec_params()` echoes the parameters it was given and the isomp4 reader leaves `channels` unset, so MP4 AAC looked like "AAC without a channel layout" and every MP4 played mute. The channel count is in the AudioSpecificConfig (`audio::asc_channels`).
- **A wrapper widget must forward `update`.** `ui::cosmetics::Placed` implemented `draw`/`layout` but not `update`, so the
  self-playing decoration inside never saw `RedrawRequested` and list decorations never moved. Any custom container
  around an `anim_image` needs `update` forwarding (regression test with the `()` null renderer, which implements
  `image::Renderer`, so widgets can be driven in unit tests).
- **Choppy animation had more causes, found by measuring (`FERROCORD_FRAME_STATS=1`)**: (1) decode caps decimated
  typical assets (a 3 s 60 fps decoration was cut to its first 160 frames and kept 1 in 4: 12.5 fps; GIF avatars
  1 in 2; delays were truncated to whole ms and clamped to >= 20 ms, so 60 fps APNGs ran 17 % slow); (2) the popout
  and hovered emoji ran on a 66 ms `time::every` tick that rebuilt the whole `view()` 15 times a second; (3) redraws of
  unrelated animations were not coalesced. Fix: caps sized to the drawn size so whole assets fit, exact `Delay`
  fractions (only <= 10 ms becomes 100 ms, like Chromium), every animation self-playing (`ui::anim_image`, with a
  `Timeline` for effect layers) and redraws rounded up to one 60 Hz grid with a process-wide origin (per-epoch
  grids interleaved: 73 redraws/s with the popout open).
- iced broadcasts `RedrawRequested` to subscriptions, but `event::listen_with` filters it out: a self-playing widget
  costs no `update`/`view`. Under Xvfb without a window manager the window is never focused, so focus-gated
  animations do not run until `xdotool windowfocus`.
- In a debug build tiny-skia's scaled, alpha-blended `draw_pixmap` dominates: the profile-effect overlay (a 170 px
  wide layer drawn at 2x over the card) costs ~300 ms per redraw unoptimised. Measure rendering with optimised
  dependencies (`--config 'profile.dev.package."*".opt-level=3'` in a separate target dir), not plain debug. Even
  optimised it is ~11 ms per redraw: tiny-skia fills an image's whole rectangle (the damage clip is only a mask), so
  any 60 fps animation inside the popout repaints the full scaled effect layers.
- A disk cache keyed by `sha256(url)` (no URL on disk) with mtime-as-LRU needs no index: `File::set_modified` on read,
  trim the oldest on write; write to a temp file and rename so a crash never leaves a truncated entry.
- **settings-proto PATCH replaces whole top-level fields**: a field sent in `PATCH /users/@me/settings-proto/1` replaces that category entirely (unsent subfields reset), omitted top-level fields stay. Send each touched category complete (read-modify-write keeps unknown fields) and nothing else. `google.protobuf.*Value` wrappers: absent = unset (client default), empty wrapper = explicit `false`/`0`.
- A process-wide display flag read by free formatting functions (12/24-hour clock) is best thread-local: iced runs `update` and `view` on one thread, and parallel tests then cannot see each other's value.
- iced `daemon::scale_factor` takes `Fn(&State, window::Id) -> f32`: a zoom setting is just state read there, no window reopen.
- **Idle animation cost**: a self-playing widget at 60 fps costs a window redraw 60 times a second however small it is (idle CPU 3.4 % -> 14 % with cosmetics on screen). The official client shows avatars and decorations in lists as stills and animates them on hover; do the same and draw a plain `image` (not a paused `AnimImage`) off-hover, so nothing is scheduled. `ui::anim_image::count_in` counts animation widgets in a view's `Tree` for tests.
- **tiny-skia `draw_pixmap` ignores the damage region**: it fills the whole (scaled) image rectangle and only masks the result, so a 60 fps avatar under a large overlay (profile effect) re-shades the whole overlay every frame. The fix belongs in ferroiced's `raster.rs` (fill only the image part that reaches the clip rectangle, padded by a device pixel; same pattern, so the same pixels). Two stock quirks make partial and full redraws differ by one unit, so pixel tests must avoid them: `draw_pixmap` truncates the image origin to whole source pixels (an image can be drawn up to a pixel outside its bounds and damage), and quads round half-way colours / anti-aliased edges differently with a clip mask (f32 pipeline) than without (8-bit).
- **Plugins: wasmi over wasmtime.** Same ABI, `word-highlighter` example, release: wasmi +1.5 MiB binary, +1.8 MiB RSS for one plugin, ~3 ms load, 70 µs per message hook; wasmtime +10.8 MiB binary, +17 MiB RSS, ~420 ms Cranelift compile per plugin, 7 µs per hook. Hooks are short and off the UI thread, so the interpreter wins on every budget that matters (RSS, cold start).
- wasmi 2.x with `default-features = false` drops `auto-dispatch` and falls back to tail-call dispatch, which needs LLVM sibling-call optimisation: in a debug build a plain `(loop (br 0))` overflows the native stack and aborts the process. Keep `auto-dispatch` (portable loop dispatch when tail calls are unsafe).
- **Never build wasmi at a different opt-level than the crate embedding it** (e.g. `[profile.dev.package.wasmi] opt-level = 3` in a dev build): on x86_64 Windows that makes host functions fault with `STATUS_ACCESS_VIOLATION` (wasmi issue #2061, `Caller::get_export`), killing the whole process; Linux was fine. Debug builds interpret ~100x slower, so tests that burn a whole fuel budget need debug-tolerant time limits.
- A host ABI with a pending-result slot (`host_call` returns a length, `host_take` copies it) avoids calling back into the guest allocator from inside a host function: no re-entrancy, trivially implementable from any language.
- Plugin "selfbot" prevention is structural: plugins have no timers, events carry no grant, and `send_message` only works with a grant minted by an explicit user action (command or menu item), scoped to its channel and spent after one message.
- **Quit hang (v0.17.0, "not responding" on tray Quit, KDE).** `iced::exit()` ends the daemon by dropping its
  tokio runtime *inside* the winit loop (window still mapped), and `Runtime::drop` waits for every
  `spawn_blocking` task with no timeout. Each notification watched its click with `wait_for_action` on the
  blocking pool; Plasma keeps notifications in its history and sends `NotificationClosed` only on dismissal,
  so one notification received earlier blocked the drop forever. gdb showed the main thread in
  `BlockingPool::shutdown` and a `tokio-rt-worker` in `ferrocord::app::wait_for_click`. Mock mode never
  notifies, so it only showed with a real account. Fix: unbounded waits on detached threads, bounded quit
  work, an exit watchdog. Reproduce with a fake notification server that never closes (Python `dbus` +
  GLib under `dbus-run-session`) and the dbusmenu `Event(id, "clicked", ...)` call a panel makes.
- **Self-update**: on Linux `std::env::current_exe()` of a binary that was renamed over reads `/path (deleted)`; restart the path the installer wrote. Windows cannot overwrite or delete a running `.exe` but can rename it, so updates move it aside to `.old` and delete it at the next start. Stage files in the target's own folder so `rename` stays atomic (no cross-filesystem copy). Safe extraction means writing files yourself from the archive entries (never `unpack`), rejecting links and `..`, and counting the bytes actually read.
- winit `dlopen`s `libxkbcommon-x11.so.0` on X11 (no link, so `ldd` and the `-dev` build packages never pull it in) and panics with exit 101 ("Library libxkbcommon-x11.so could not be loaded") when it is missing. Stock GitHub runners lack it: CI that starts the real binary under Xvfb must install `libxkbcommon-x11-0`. Reproduce by bind-mounting `/dev/null` over the library in `unshare -m`.
- **"Texts got broken" (boxes for names, timestamps, headers; blank digits in messages).** Two causes together.
  (1) `iced::font::load` is executed by iced_winit only when a compositor exists (`if let Some(compositor)`), and an
  `iced::daemon` creates it with the first window: started with `--minimized` / autostart, the boot `font::load`
  tasks were silently dropped and Inter never existed (unnoticed on machines with a system Inter, e.g. Debian's
  `fonts-inter`). Load bundled fonts into `font_system().write().raw()` in `main` instead. (2) With the requested
  face missing, cosmic-text's Basic shaping (iced `Shaping::Auto` for ASCII text) takes the FIRST fallback font
  without checking coverage; Twemoji was first, has only `#*0-9`, so letters became `.notdef` boxes and digits its
  blank keycap bases (Advanced shaping fell through to other fonts, so non-ASCII names showed in DejaVu/mono).
  A face whose PostScript name contains "Emoji" matches every style in cosmic-text, so it also outranked a missing
  italic. Keep the UI family first in the fallback and bundle every style the views use. Reproduce with
  `FONTCONFIG_FILE` pointing at a conf with only DejaVu, `--mock --minimized` under `dbus-run-session` (no tray
  host, so the window opens later).
- **"Not all members show up" (v0.20.0).** Four causes, each reproduced with a reducer test: the sidebar never
  subscribed past row 499 (five fixed ranges) and only drew loaded rows; a channel switch in the same server kept the
  previous channel's rows and applied that list's later INSERT/DELETEs (lists are per *list id*: `everyone` or a hash of
  the channel's view overwrites); INSERTs were dropped once 1500 rows existed, shifting every later row; and one
  unreadable member item failed the whole `GUILD_MEMBER_LIST_UPDATE`. Match the official client: `[0,99]` plus the chunks
  at the scroll position (three ranges), the length from `groups`, a sparse map of the subscribed ranges, fixed row
  heights so the visible window is plain arithmetic, and per-item lenient parsing that keeps every slot.
- Group DMs change members through `CHANNEL_RECIPIENT_ADD/REMOVE`, not through the channel object: without them a
  person added to a group appeared only after a restart.
