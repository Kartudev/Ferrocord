# Protocol

Ferrocord talks to Discord the way a browser session of the official client
does: a user-account **Gateway** WebSocket for real-time events, the **REST
API** for history and actions, and the **remote-auth gateway** for QR login.
There is no official SDK for user accounts; everything below is implemented in
`src/net/` on top of `tokio-tungstenite` and `reqwest`.

> Third-party clients break Discord's Terms of Service. See the README warning.

## 1. Library choice

| Need | Choice | Why |
|---|---|---|
| WebSocket | `tokio-tungstenite` (native-tls) | Mandated by the brief; mature, tokio-native. |
| HTTP | `reqwest` (native-tls, JSON) | Mandated by the brief; connection pooling, timeouts. |
| zlib-stream | `flate2` | One shared inflate context per connection. |
| QR login crypto | `rsa` (OAEP/SHA-256), `sha2`, `base64` | Required by the remote-auth handshake. |
| QR rendering | `qrcode` | Module matrix → RGBA image in the login view. |

Bot libraries (serenity, twilight) were not used: they target bot accounts,
intents and sharding, and do not model user-account READY payloads, read
states or remote auth.

## 2. Gateway (`net/gateway.rs`)

* URL: `wss://gateway.discord.gg/?v=10&encoding=json&compress=zlib-stream`
  (`FERROCORD_GATEWAY_URL` overrides the host; `FERROCORD_ZLIB=0` disables
  compression).
* Architecture: a pure state machine `Session` (`on_payload`, `heartbeat`,
  `connect_url`) returns `Action`s; the async runner owns the socket, timers
  and reconnect loop. The stream returned by `gateway::spawn` *is* the
  connection: dropping it (logout, quit) closes the socket with code 1000.

### Opcodes handled

| Op | Name | Direction | Handling |
|---|---|---|---|
| 0 | DISPATCH | ← | Tracks `s`; parsed by `types::parse_dispatch`. |
| 1 | HEARTBEAT | ↔ | Sent every `heartbeat_interval`; answered immediately when the server asks. |
| 2 | IDENTIFY | → | Token, `capabilities`, `properties` (honest: `browser = "Ferrocord"`; same object as `X-Super-Properties`), presence. |
| 3 | PRESENCE_UPDATE | → | Own status (`online`/`idle`/`dnd`/`invisible`) plus the custom status as an activity (`{type: 4, name: "Custom Status", state, emoji}`); also saved in the settings proto (§14). IDENTIFY carries the same presence. |
| 14 | GUILD_SUBSCRIPTIONS | → | Member list ranges `{guild_id, typing, threads, activities, channels: {id: [[0,99], ...]}}` for the open channel, sent while the member sidebar is open (at most three 100-row ranges, see below). |
| 6 | RESUME | → | `{ token, session_id, seq }` on the `resume_gateway_url`. |
| 7 | RECONNECT | ← | Close with 4000 and resume. |
| 9 | INVALID_SESSION | ← | `d = true` → resume; `false` → reset session, wait 1–5 s, identify. |
| 10 | HELLO | ← | Start heartbeating (first beat after `interval × random[0,1)`), then IDENTIFY or RESUME. |
| 11 | HEARTBEAT_ACK | ← | Clears the pending flag, measures latency. |

### Commands from the app (`GatewayHandle`)

The app records what it wants (own status, member list ranges) in a shared
`GatewayHandle`; the runner sends op 3 / op 14 once the session is READY and
again after every reconnect or resume. Identical requests are not re-sent.
Presence comes from READY `presences` (friends), `PRESENCE_UPDATE`, and member
list items.

### Member list (`app/members.rs`, `ui/members.rs`)

Like the official client:

* **Ranges follow the scroll position.** The sidebar always subscribes to
  `[0, 99]` plus the 100-row chunks at the visible window (with a 30-row
  preload margin), three ranges at most: `[[0,99]]` at the top,
  `[[0,99],[2500,2599]]` (and the next chunk near its end) deep in a big list.
  A scroll that changes the chunks re-sends op 14 for the same channel.
* **The list knows its length, not its rows.** Every
  `GUILD_MEMBER_LIST_UPDATE` carries `groups` (`[{id, count}]`: hoisted
  roles, `online`, `offline`); total rows = members + one header per group.
  Rows are kept in a sparse map and only inside the subscribed ranges, so a
  100 000-member server costs as much as a small one. Discord itself leaves the
  offline group out for servers of about 1000 members or more (same as the
  official client: only online members are listed there).
* **Ops**: `SYNC {range, items}` fills a range; `INSERT {index, item}` /
  `DELETE {index}` shift every later row by one (a moved member is a DELETE
  plus an INSERT in one update); `UPDATE {index, item}` replaces a row;
  `INVALIDATE {range}` forgets a range (it shows placeholders again). An item
  we cannot read keeps its slot (an empty item), and an unreadable item never
  drops the whole update: one odd member used to fail the event, so a whole
  SYNC (every member of a range) was lost.
* **List ids.** `id` is `everyone` or a hash of the channel's view
  permissions; channels with different permissions have different lists.
  After a channel switch in the same server the rows stay until the first
  SYNC: a SYNC of another list replaces them, and later updates of the old
  list are ignored. Ops for a list that never had a SYNC are ignored.
* **Drawing** is virtualised (`ui::members::ROWS`): member rows are 44 px and
  group headers 40 px, so the visible window is computed from the scroll
  offset; rows not loaded yet are grey skeleton rows of the same height.

Fixed after 0.20.0 (user report "not all members show up"): the old list asked
for at most rows 0-499 (`ranges_for` capped at five ranges) and showed only
loaded rows, so members past row 500 never appeared; it kept the previous
channel's rows and applied the old list's updates after a channel switch
(members of a private channel mixed with `everyone`); it dropped INSERTs once
1500 rows existed (every later row one place off); and one unreadable member
item failed the whole update.

**Group DMs** have no member list subscription: the panel lists the channel's
`recipients` (plus the current user), kept current by
`CHANNEL_RECIPIENT_ADD` / `CHANNEL_RECIPIENT_REMOVE` (`{channel_id, user}`;
previously ignored, so people added to a group only showed after a restart).
A recipient id without a user object gets an "Unknown User" placeholder until
a payload carries the user, instead of disappearing.

### Heartbeat, zombies, reconnect

* If a heartbeat is due while the previous one was never acknowledged the
  connection is a zombie: close with a non-1000 code and resume.
* Reconnect delay: first retry after a healthy session within 0.1–1 s,
  then exponential backoff `1 s · 2^(n-1)`, capped at 60 s, with jitter
  factor `[0.5, 1)` (`gateway::backoff`). A successful READY/RESUMED resets
  the attempt counter. The UI shows `Reconnecting { attempt, delay_ms }`.
* Close codes: 4004 → `AuthFailed` (session cleared, back to login);
  4010–4014 → fatal; 4007/4009 → new session; everything else → resume if a
  session exists.

### zlib-stream (`net/zlib.rs`)

Binary frames are fragments of a single zlib stream. They are buffered until
the `00 00 ff ff` suffix, then inflated with `FlushDecompress::Sync` on the
shared `Decompress` context. The buffer is capped at 64 MiB.

### Dispatch events consumed

`READY`, `READY_SUPPLEMENTAL`, `RESUMED`, `MESSAGE_CREATE`, `MESSAGE_UPDATE`
(partial), `MESSAGE_DELETE`, `MESSAGE_DELETE_BULK`, `TYPING_START`,
`MESSAGE_ACK`, `CHANNEL_CREATE/UPDATE/DELETE`, `THREAD_CREATE/UPDATE/DELETE`,
`CHANNEL_RECIPIENT_ADD/REMOVE`, `GUILD_CREATE/UPDATE/DELETE`,
`MESSAGE_REACTION_ADD/REMOVE/REMOVE_ALL/REMOVE_EMOJI` (with `burst`),
`PRESENCE_UPDATE`, `GUILD_MEMBER_LIST_UPDATE`, `GUILD_EMOJIS_UPDATE`, `GUILD_STICKERS_UPDATE`,
`GUILD_ROLE_CREATE/UPDATE/DELETE`, `GUILD_MEMBER_UPDATE` (member roles),
`RELATIONSHIP_ADD/UPDATE/REMOVE`. Unknown events are ignored; a malformed known
event is logged (without its payload) and the connection stays up.

### User-account READY quirks

The parser (`WireReady::into_domain`) accepts both layouts that the
capability flags produce:

* guild name/icon either top-level or under `properties`;
* `users` (deduplicated) plus inline `recipients`, and `recipient_ids`;
* `read_state` as a plain array or `{ entries, partial, version }`;
  only `read_state_type == 0` (channels) is used; `last_message_id: 0` means
  never read;
* `user_guild_settings` (muted guilds / channel overrides) and
  `user_settings.guild_folders` / `guild_positions` for the rail order;
* `merged_members[i]` aligned with `guilds[i]` for the current user's roles
  (role mentions, author colours). `READY_SUPPLEMENTAL` adds other members.
* Unavailable guilds are skipped until their `GUILD_CREATE`.
* `relationships` (type 1 friend, 2 blocked, 3 incoming, 4 outgoing,
  5 implicit; inline `user`) feed the Friends page; DMs with
  `is_message_request` / `is_spam` go to Message Requests, not the DM list.

Message timestamps come from the snowflake id (`id >> 22` + Discord epoch),
so no ISO-8601 parsing is needed on the hot path.

## 3. REST (`net/rest.rs`, `net/ratelimit.rs`)

Base: `https://discord.com/api/v9` (`FERROCORD_API_URL`). Header
`Authorization: <token>` (user tokens have no `Bot ` prefix) and an honest
`User-Agent: Ferrocord/<version>`. Every request also carries the headers the
official clients send (`net/identity.rs`), with honest values:

| Header | Value |
|---|---|
| `X-Super-Properties` | base64 of the same JSON as IDENTIFY's `properties`: `os` (`Windows`/`Linux`), `browser: "Ferrocord"`, `device: ""`, `system_locale`, `browser_user_agent` (= the `User-Agent`), `browser_version`, `os_version` (kernel release on Linux), `release_channel: "stable"`, `client_version`. No `client_build_number`: Ferrocord does not claim to be Discord's build. |
| `X-Discord-Locale` | system UI language (`en-US`, `sys-locale`) |
| `X-Discord-Timezone` | IANA zone (`Europe/Madrid`, `iana-time-zone`), omitted if unknown |
| `Cookie` | what Discord sets (`__dcfduid`, `__sdcfduid`, `__cfruid`), in-memory jar, never written to disk |

While logged out, `GET /experiments` gives a `fingerprint` that the login
calls send as `X-Fingerprint` (the official client does the same).

**Captcha.** A 400 body with `captcha_key` becomes
`Error::CaptchaRequired(CaptchaChallenge)` (`net/captcha.rs`: `captcha_key`,
`captcha_sitekey`, `captcha_service`, `captcha_rqdata`, `captcha_rqtoken`,
`captcha_session_id`; lenient, size-capped, header-safe). A retry carries the
user's solution as `X-Captcha-Key` plus `X-Captcha-Rqtoken` /
`X-Captcha-Session-Id` (`CaptchaSolution::headers`), exactly the official
client's retry. Ferrocord never solves a captcha and uses no solver service.

| Call | Endpoint |
|---|---|
| Validate token | `GET /users/@me` |
| History | `GET /channels/{id}/messages?limit=50[&before=]` |
| Send | `POST /channels/{id}/messages` `{ content, nonce, tts }` |
| Typing | `POST /channels/{id}/typing` (at most every 8 s while typing) |
| Edit / delete | `PATCH` / `DELETE /channels/{c}/messages/{m}` |
| Reply | `POST .../messages` with `message_reference` and, if "@ OFF", `allowed_mentions {parse:[users,roles,everyone], replied_user:false}` |
| Reactions | `PUT/DELETE /channels/{c}/messages/{m}/reactions/{emoji}/@me` (`%`-encoded Unicode, or `name:id`) |
| Who reacted | `GET /channels/{c}/messages/{m}/reactions/{emoji}?limit=&type=0\|1[&after=]` (`type=1`: burst / super reactions; users in id order, `after` = last user id). Only on hover / dialog, see §8. |
| Jump | `GET .../messages?around={m}` / `?after={m}` |
| Search | `GET /guilds/{g}/messages/search?content=&offset=` or `/channels/{c}/...` (202 + `retry_after` while indexing is retried, max 4 tries) |
| Upload | `POST .../messages` `multipart/form-data`: `payload_json` + `files[n]`, streamed from disk with an explicit `Content-Length` |
| Status | `PATCH /users/@me/settings-proto/1` (`status.status`, `status.custom_status`; §14) |
| User settings | `GET`/`PATCH /users/@me/settings-proto/1` (§14) |
| Profile edit | `PATCH /users/@me {global_name}`, `PATCH /users/@me/profile {pronouns, bio}` |
| Mark read | `POST /channels/{c}/messages/{m}/ack` `{ token: null }` |
| QR ticket | `POST /users/@me/remote-auth/login` `{ ticket }` with `X-Fingerprint` |
| Fingerprint | `GET /experiments` (logged out) → `fingerprint` |
| GIF picker | `GET /gifs/trending-gifs` / `GET /gifs/search?q=` with `media_format=mp4&provider=tenor&locale=en-US` (`{url, src, gif_src, preview, width, height}`); picking sends the Tenor `url` as text (shown back as a `gifv` embed). Opens on Favorites / Trending GIFs tiles like the official picker. Stills: `preview`, else `gif_src`, from whatever HTTPS host the API names (Tenor uses several), with a plain client (no token, no Discord headers or cookies, 10 MB cap), decoded to a <= 256x400 first frame off the UI thread, own 12 MB LRU (`App::gif_thumbs`). Tiles animate (`app/gif_anim.rs`): Tenor's tile-sized `tinygif` (`AAAAM`), else `gif_src`, else Giphy's `200w.gif`, decoded in-process to tile-sized frames; an MP4-only saved GIF goes through the video helper's frames mode. With "Automatically play GIFs" off only the hovered tile plays; "Reduce motion" shows stills. Only tiles near the visible part of the grid are decoded (40 MB budget, frames of tiles a screen away dropped, all freed on close). Every picker download goes through a 100 MB on-disk LRU (`<cache dir>/gifs`, SHA-256 of the URL as file name, at most 4 downloads at a time); the saved GIFs are prefetched into it the first time the picker opens in a session. |
| Recent emoji | Local only, no request: `meta("pref.recent_emoji")` (JSON list of `Emoji`, newest first, no duplicates, at most 36; kept on logout) in `app/emoji.rs::Recent`. Updated when an emoji is picked, added as a reaction (not when removed) or written in a sent message (`<:name:id>` / `<a:name:id>` and Unicode, normalised to the fully qualified form). The picker opens on "Recently Used" when the list is not empty; the message hover bar offers the first 3 the user can react with in that channel (Nitro-locked and unknown custom emoji skipped). Discord's own emoji frecency (`FrecencyUserSettings`) is not read or written. |
| Favourite GIFs | `GET /users/@me/settings-proto/2` -> `{settings: base64}` = `FrecencyUserSettings` (discord-protos): field 2 `FavoriteGIFs { map<string, FavoriteGIF> gifs = 1; bool hide_tooltip = 2 }`, `FavoriteGIF { GIFType format = 1 (1 image, 2 video); string src = 2; uint32 width = 3, height = 4, order = 5 }`, key = Tenor page URL, `src` = the MP4 (format 2). The star does GET, changes one map entry, `PATCH /users/@me/settings-proto/2 {settings: base64}` with the whole message (every other field and unknown fields copied byte for byte, `net/frecency.rs`); saves are serialised, optimistic and rolled back on failure. Loaded once, when the picker first opens. |
| Sticker | `POST .../messages` `{ content: "", nonce, sticker_ids: [id] }` |
| Media | CDN `avatars/`, `icons/`, `emojis/`, `stickers/{id}.png\|.gif?size=160`, attachment `proxy_url?width=&height=`; GIF picker previews from `media.tenor.com` (HTTPS, never with the token) |

### Rate limits

* Route key = method + path with major parameters (`channels/`, `guilds/`,
  `webhooks/` ids) kept and other ids replaced by `:id`
  (`ratelimit::route_key`).
* `X-RateLimit-Bucket` maps a route to a bucket; bucket state is
  `remaining` + reset instant (`X-RateLimit-Reset-After`), keyed by
  `hash:major`. A request with `remaining == 0` waits until the reset.
* 429: wait `retry_after` from the JSON body (falling back to the
  `Retry-After` header), global if `X-RateLimit-Global` or `global: true`;
  retry up to 3 times.
* The limiter takes the clock as a parameter and is unit-tested; the mock
  REST tests check real waiting behaviour.

### Optimistic sending

The composer creates a pending message with a snowflake `nonce`; the
`MESSAGE_CREATE` echo (or the REST response) with the same nonce replaces
it. Failures turn it red with "Message could not be delivered.".

## 4. QR login / remote auth (`net/remote_auth.rs`)

`wss://remote-auth-gateway.discord.gg/?v=2` with `Origin: https://discord.com`.

1. ← `hello { heartbeat_interval, timeout_ms }`
2. → `init { encoded_public_key }` — fresh RSA-2048 key, SPKI DER, base64
3. ← `nonce_proof { encrypted_nonce }` (RSA-OAEP, SHA-256)
4. → `nonce_proof { proof }` = base64url-no-pad(SHA-256(nonce))
5. ← `pending_remote_init { fingerprint }` → QR of `https://discord.com/ra/<fingerprint>`
6. ← `pending_ticket { encrypted_user_payload }` → `id:discriminator:avatar:username`
7. ← `pending_login { ticket }` → REST ticket exchange → `encrypted_token` → decrypt
8. ← `cancel` when declined on the phone; the socket closes at `timeout_ms`.

Client heartbeats are `{ "op": "heartbeat" }`. The protocol is implemented
and tested end to end against a local mock (`tests/mock_remote_auth.rs`).
`v=2` and the `Origin` match the official web client.

### Captcha on the QR login

Users reported "QR login unavailable — Discord wants you to solve a captcha".
The only REST call of the QR flow is the ticket exchange, so that is where
Discord asks (the gateway itself cannot answer with a captcha). Before v0.12
that request was much barer than the official client's: no
`X-Super-Properties`, no `X-Fingerprint` (no `GET /experiments` first), no
Discord cookies, no locale/time zone: all signals Discord's risk scoring
uses on the request that creates a new session. Those are now sent (above).
Discord can still demand a captcha (new IP, VPN, account history), so:

1. `RaEvent::CaptchaRequired` replaces the dead-end error;
2. the app opens **Discord's own login page** (§4.1) automatically. The ticket
   is bound to the QR socket and the challenge only works on discord.com, so
   the user finishes there (scan again, or email + password) and solves the
   captcha Discord shows.

### 4.1 Log in with Discord's page (`auth/web_login.rs`, `ferrocord-login`)

Also a button on the login screen. The `ferrocord-login` helper (a separate
binary: the app links no browser engine) opens `https://discord.com/login` in
the system webview via `wry`/`tao`: WebView2 on Windows, WebKitGTK 4.1 on
Linux. Options considered:

| Option | Verdict |
|---|---|
| (a) our own page with the hCaptcha widget (Discord's sitekey + rqdata) in a webview | Rejected: hCaptcha only runs on the sitekey's allowed domains; a `file://`, custom-scheme or `127.0.0.1` page fails its hostname check, and making it pass would mean faking the origin. |
| (c) the same page in the system browser on a loopback callback | Rejected for the same reason (localhost is not Discord's domain). |
| (b) Discord's own login page in a webview | **Chosen**: the user sees exactly Discord's pages; captcha, 2FA, passkeys and its QR code all work as in a browser, with no Ferrocord code in the loop. |

* Profile: incognito, inside a fresh `0700` temp directory
  (`FERROCORD_LOGIN_DATA_DIR`); the app wipes it after the helper exits (with
  retries: WebView2 child processes hold files briefly).
* Navigation: top-level HTTPS only; pop-ups refused. The browser's own
  User-Agent is untouched.
* Token: a document-start script, active only in the top-level
  `https://discord.com` document, keeps a reference to the page's
  `localStorage` and posts the `token` the page stores after the user logged
  in, once, over wry IPC. The helper re-checks the sender URL and the token
  shape, prints it on its stdout pipe and exits. The script only reads.
* App side: the token is validated with `GET /users/@me` and stored only in
  the keyring, like a QR login.
* Wire format (stdout lines): `{"type":"ready"}`, `{"type":"token","token"}`,
  `{"type":"closed"}`, `{"type":"error","code":"no-webview"|"failed","message"}`.
* Missing engine: Linux loader errors (`libwebkit2gtk-4.1.so.0`, exit 127)
  and WebView2 creation errors become "needs WebKitGTK 4.1 / WebView2" with
  package names; QR login keeps working.

Verified in CI: parsing, headers, retry headers, the state machine and the
helper protocol (with fake helpers), and that only `ferrocord-login` links
WebKitGTK. Needs a real account: Discord's page, its captcha and the token
hand-over.

## 5. Read states, unread and mentions (`app/state.rs`)

* A channel is unread when `channel.last_message_id > read_state.last_read`
  (or it has messages and no read state).
* `mention_count` comes from READY and is incremented locally for messages
  that mention the user, their roles, `@everyone`/`@here`, or any DM message
  from someone else. `MESSAGE_ACK` from other sessions updates both.
* Opening a channel (or receiving a message while it is open and the window
  focused) marks it read locally and ACKs it over REST.
* Muted guilds/channels do not show the unread pill, but mention badges
  still count — like the official client.
* Desktop notifications (`notify-rust`, D-Bus) fire for mentions and DMs
  unless the conversation is open and focused. `FERROCORD_NOTIFY_PREVIEW=0`
  shows "New message" instead of the text.

## 6. Local cache (`store/`)

SQLite (`cache.sqlite3`, WAL). Tables: `meta` (selected guild/channel,
`me`), `users`, `guilds`, `channels`, `messages` (≤ 1000 per channel),
`read_states`. Domain objects are stored as JSON next to indexed ids.
Pending/failed local messages and credentials are never written. The
database is plain text (not encrypted); the data directory is `0700` and the
database, `-wal`/`-shm` and log files are `0600`.

## 7. UI reference

Measurements of the official dark theme reproduced in `ui/theme.rs`:

| Element | Value |
|---|---|
| Server rail | 72 px wide, `#1e1f22`, icons 48 px, 8 px gap, circle → 16 px radius on hover/selection |
| Rail pill | 4 px wide white; 8 px tall unread, 40 px selected |
| Channel sidebar | 240 px, `#2b2d31`; header 48 px with a darker bottom edge (gradient, see below) |
| Category | 12 px semibold uppercase `#949ba4`, chevron |
| Channel row | 32 px tall, radius 4, name 16 px medium; muted `#80848e`, unread white semibold, selected `rgba(78,80,88,.6)` |
| Mention badge | `#f23f43`, 16 px tall, 12 px bold white |
| Main pane | `#313338`; header 48 px, 16 px semibold title, topic `#b5bac1` |
| Message group | avatar 40 px at 16 px inset, name 16 px medium (role colour), timestamp 12 px `#949ba4`, 16 px top gap, grouped within 7 min |
| Mention row | background `rgba(240,177,50,.08)`, 2 px `#f0b132` left bar |
| Mention chip | `rgba(88,101,242,.3)` / `#c9cdfb` |
| Embed | `#2b2d31`, 4 px colour bar, max 520 px, 16 px padding |
| Composer | `#383a40`, radius 8, 44 px; typing indicator 24 px below |
| User panel | 52 px, `#232428` |
| Accent | `#5865f2`, links `#00a8fc` |

Shadows are safe with ferroiced (tiny-skia clips them to the damage region), but
no style uses `container::Style::shadow`: header "shadows" are a gradient on the
bar itself and popups rely on their border, as in the official client.

Fonts: Inter (OFL) and Twemoji (CC-BY 4.0, COLRv0 font) embedded. Icons: Lucide
(ISC), plus our own `gif.svg` in the same style. No Discord logos, fonts or artwork are used.

### Friends, Message Requests, switcher (`app/friends.rs`, `ui/friends.rs`, `net/rest/friends.rs`)

* Friends page (Home with no DM open): Online / All / Implicit / Pending
  tabs with a name or username search, and Add Friend. Pending lists
  incoming requests first; the red badge (Friends entry, Pending tab)
  counts incoming requests. Removing a friend asks once in the row.
* Calls: `POST /users/@me/relationships {username, discriminator: null}`
  (Add Friend; a 400 means no such user, a captcha demand is shown, never
  solved), `PUT /users/@me/relationships/{id} {}` (accept / add implicit),
  `DELETE /users/@me/relationships/{id}` (remove, ignore, cancel),
  `POST /users/@me/channels {recipients}` (Message, when no DM exists).
* Message Requests: Requests / Spam tabs; Accept is
  `PUT /channels/{id}/recipients/@me {consent_status: 2}`, Ignore is
  `DELETE /channels/{id}/recipients/@me`. Messages in a request neither
  badge nor notify. Not verified against a live account yet.
* "Find or start a conversation" (button or Ctrl+K): DMs, friends without a
  DM, text channels and servers; Enter opens the first result.

## 8. Round 2 behaviour notes

* **Who reacted** (`app/reactions.rs`, `ui/reactions.rs`): hovering a
  reaction pill shows the official tooltip, "Alice, Bob, and 3 others reacted
  with :thumbsup:" (word shortcodes, `:name:` for custom emoji), after a
  300 ms rest. Only then are the first 3 reactors fetched (never for every
  message on screen); leaving earlier fetches nothing (a generation counter
  drops the timer). Right-click on a pill, or "Reactions" in the message menu,
  opens the dialog: one tab per emoji (burst reactions get their own tab,
  `count_details.burst`), 100 users per page with "Load more" (up to 1000).
  Lists are cached in a 64-entry LRU; `MESSAGE_REACTION_ADD/REMOVE` patch a
  complete list in place (known user) or mark it stale (refetched on next
  hover), `REMOVE_EMOJI` / `REMOVE_ALL` / `MESSAGE_DELETE` drop it. All calls go
  through the REST layer's buckets (reaction routes share one `:emoji` bucket).

* **Replies**: header from `referenced_message` (null = "Original message was
  deleted"); refreshed from the loaded window when the original is there.
  Replying to yourself never pings.
* **Upload limits** (`rest::upload_limit`): 10 MiB default; Nitro Classic/Basic
  50 MiB, Nitro 500 MiB; server boost tier 2 = 50 MiB, tier 3 = 100 MiB;
  at most 10 files. Pasted images are written to a private temp file and
  deleted afterwards.
* **Detached windows**: after jumping to an old message the window holds that
  region only; live messages are not appended until "Jump To Present" or
  scrolling to the bottom loads the newer pages.
* **Desktop helpers**: file dialog via `rfd` (xdg-portal, no GTK); Ctrl+V reads
  copied files or an image through `arboard` (X11 and Wayland data-control),
  encoded to PNG with a pixel and byte cap.

## 9. Round 3: header buttons, servers, discovery, folders

All calls go through `RestClient::request` (per-route buckets, 429 retry);
pure parsing/payloads live in `net/extras.rs`, calls in `net/rest/ext.rs`.

| Feature | Calls |
|---|---|
| Bell | `PATCH /users/@me/guilds/{guild\|@me}/settings` with `channel_overrides.{id}`: `message_notifications` (0 all, 1 mentions, 2 nothing, 3 inherit), `muted` + `mute_config {selected_time_window, end_time}` (`-1`/`null` = until turned back on). READY `user_guild_settings` and `USER_GUILD_SETTINGS_UPDATE` feed `ChatState::channel_notify`; expired timed mutes are dropped. |
| Pins | `GET /channels/{id}/pins` (array, or the paginated `{items:[{message}]}` shape); `DELETE /channels/{c}/pins/{m}`. The unpin button shows for DMs and where the user has Manage Messages in that channel (owner, Administrator, roles, channel overwrites; see §15). A 403 is still reported. |
| Inbox | `GET /users/@me/mentions?limit=25&roles=true&everyone=true[&before=][&guild_id=]` (Mentions tab, "This Server" filter, load more). |
| Help | opens `https://support.discord.com/` in the browser. |
| Search | `GET /guilds/{id}/messages/search` / `/channels/{id}/messages/search`; `from:name` -> `author_id`, `has:image\|video\|sound\|file\|link\|embed\|sticker` -> `has`, `in:#channel` -> `channel_id` (servers only). Names resolve against known users/channels; an unknown one is an error shown in the panel. |
| Create server | `POST /guilds {name, icon: uri|null, channels: [], system_channel_id: null, guild_template_code: "2TffvPucqHkN"}`; icon is a PNG/JPEG/GIF/WebP `data:` URI (4 MB cap, no cropping). The template code is Discord's "Blank Server" (what the official client's "Create My Own" sends); without it user accounts get HTTP 403. |
| Join | `GET /invites/{code}?with_counts=true&with_expiration=true` (preview card) then `POST /invites/{code} {session_id}`. A 400 body with `captcha_key` becomes `Error::CaptchaRequired`: a clear message, one attempt, **no solving**. |
| Discover | `GET /discovery/categories`, `GET /discoverable-guilds?offset&limit[&category_id]`, `GET /discovery/search?query&offset&limit[&category_id]`, join with `PUT /guilds/{id}/members/@me {session_id}`. "Details" is a card from the listing (no lurking). Endpoint shapes are tolerant (`guilds`/`hits`/array). |
| Folders | READY `user_settings.guild_folders` (`id`, `name`, `color`, `guild_ids`; `id: null` = single server). Saved with `PATCH /users/@me/settings {guild_folders: [...]}` (the same legacy settings endpoint that stores the status). Ferrocord identifies with `capabilities: 0`, so the server keeps sending legacy JSON settings; the official client's `PATCH /users/@me/settings-proto/1` (base64 protobuf `GuildFolders`) would need the proto capability and an encoder, and is not implemented. `USER_SETTINGS_UPDATE` with `guild_folders` keeps other sessions in sync; a failed save rolls back. The list is cached in `meta("guild_folders")`. |

Rail gestures (`app/rail.rs`): press on a server, release on another -> new
folder in place; release on a folder -> joins it; release on the same item ->
click; a release anywhere else (`DragEnd`) cancels. Folders open/closed state
is local only.

## 10. Profile cosmetics and animations

Field names and URLs were checked against the public user-account docs
(discord-userdoccers `resources/user`, `resources/collectibles`,
`resources/presence`, `reference#cdn-formatting`) and Discord's official API
docs. What could not be confirmed against a live account is listed at the end.

### Payload fields (`net/cosmetics.rs`)

All of these are parsed leniently (`lenient`): a wrong shape drops the field,
never the message or member list.

| Where | Field | Use |
|---|---|---|
| user / partial user / member | `avatar_decoration_data { asset, sku_id }` | frame over the avatar |
| user | `collectibles.nameplate { asset, palette, label, sku_id }` | row background |
| user | `primary_guild { identity_guild_id, identity_enabled, tag, badge }` | server tag chip |
| user | `display_name_styles { font_id, effect_id, colors[] }` | name colour / gradient |
| user | `banner`, `accent_color` | popout banner |
| presence | `activities[]` (type, name, details, state, emoji, timestamps, assets, application_id) | popout activity and custom status |

`GUILD_MEMBER_LIST_UPDATE` member items may carry the member's own
`avatar_decoration_data`; it replaces the user's. The member-list
subscription (op 14) now sends `"activities": true`, like the official
client, so presences include activities.

### Profile popout

`GET /users/{id}/profile?type=popout&with_mutual_guilds=true&with_mutual_friends=true&with_mutual_friends_count=true[&guild_id=..]`.
The body's `user_profile` and `guild_member_profile` are layered (the guild
value wins when set; an empty guild bio / pronouns falls through). Used:
`bio`, `pronouns`, `banner`, `accent_color`, `theme_colors[2]`,
`profile_effect.id`, `badges[]`, `premium_since`, `guild_member`
(`roles`, `joined_at`), `mutual_guilds[]`, `mutual_friends[]`,
`mutual_friends_count`. Profiles are cached (48, LRU) and refreshed when
older than 5 minutes; a failed refresh keeps the old card.

### Profile effects

`GET /user-profile-effects` (once, lazily, when a shown profile has an
effect) returns the layer configs: `src`, `loop`, `width`, `height`,
`duration`, `start`, `loopDelay`, `position {x,y}`, `zIndex`,
`randomizedSources[]`. Layers are drawn over the popout card with the
timeline in `net::cosmetics::layer_time` (one-shot layers end, looping ones
repeat after `loopDelay`), scaled so the union of the layer rectangles spans
the card width. Frames are decoded at a reduced width (256 px) and scaled on
draw; they are dropped when the popout closes.

### CDN URLs

| Asset | URL |
|---|---|
| avatar (still) | `/avatars/{uid}/{hash}.png?size=80` (160 in the popout) |
| avatar (animated, `a_`) | same with `.gif` |
| decoration | `/avatar-decoration-presets/{asset}.png?size=96&passthrough=false` (still) / `passthrough=true` (APNG, `a_` assets) |
| banner | `/banners/{uid}/{hash}.png|gif?size=600`; member banner `/guilds/{gid}/users/{uid}/banners/{hash}` |
| nameplate | `/assets/collectibles/{asset}static.png` (the animated variant is `asset.webm`: not decoded) |
| server tag badge | `/guild-tag-badges/{gid}/{badge}.png?size=32` |
| profile badge | `/badge-icons/{icon}.png` (fetched from the CDN, not bundled; a neutral dot shows until loaded) |
| activity art | application asset `/app-assets/{app}/{id}.png`, `mp:` -> `media.discordapp.net`, `spotify:` -> `i.scdn.co/image` |
| animated emoji | `/emojis/{id}.gif?size=96` |
| emoji "Send as image" | `/emojis/{id}.png\|.gif?size=48&quality=lossless`, scaled to fit 48 px, uploaded as an attachment |
| sticker "Send as image" | `/stickers/{id}.png\|.gif?size=160`, scaled to fit 160 px (APNG becomes an animated GIF) |

`i.scdn.co` was added to the media host allow-list for Spotify album art;
downloads never carry the token.

### When cosmetics animate (official policy)

Outside profiles, animated avatars and avatar decorations are stills (the
`.png` / `passthrough=false` URLs) and play only while their own row is
hovered: a message row (chat), a member-list row, a DM-list row, the user
panel's identity area (`App::list_phase`, `anim::Hover` with its `Zone`). The
animated asset is fetched on the first hover, like the official client,
which asks for the `a_` asset only when the component may animate. The
profile popout (and nothing else) animates avatar, decoration, banner and
effect without a hover. Animated emoji and GIFs in messages are unchanged
(they follow "Play GIFs automatically"); "Reduce motion" keeps everything
still. Sources: Discord's own client gates avatar and decoration animation
per component (`canAnimate`, `isAvatarDecorationAnimating`,
`onlyAnimateOnHoverOrFocus` props, visible in the client mods that patch
them, e.g. Vencord's `AlwaysAnimate` and `Decor` plugins), and the Discord
support community has long-standing requests to make animated avatars play
without hovering. A still draws no animation widget, so an idle window with
cosmetics on screen asks for no redraws.

### Animation engine (`app/anim.rs`)

* GIF, APNG and animated WebP are decoded in `spawn_blocking` with the
  `image` crate; single-frame images are left to the static path.
* Caps per asset (source frames read, frames kept, bytes kept, width) are
  per `Class`. Frames are decoded at about 1.5-2x their drawn size and every
  source frame is read (up to 600), so a typical asset (a 3 s 60 fps
  decoration, a 2 s GIF avatar or emoji) keeps all of its frames and plays
  at its own rate; only unusually long or large ones are decimated (evenly:
  merged frames add their delay, so the duration is unchanged). Delays are
  exact (APNG / WebP fractions such as 1/60 s); only "as fast as possible"
  delays (<= 10 ms) become 100 ms, as in Chromium. Sources above 2048 px
  are refused.
* `AnimCache` is an LRU with an 80 MB budget. Banner and effect frames are
  dropped when the popout closes.
* Nothing ticks. Every animation (hovered list / message avatars and
  decorations, the popout's avatar, decoration, banner and effect layers, message media,
  hover-played emoji, GIF-picker tiles) is an `ui::anim_image::AnimImage`
  with a start time (`App::anim_clock`: the app epoch, the popout opening
  or the hover start) and a playing flag (window focused, motion on).
  Effect layers carry their `Timeline` (start, duration, loop delay).
  "Reduce motion" in Settings turns all of it off.
* Handles are created once when data arrives (`Handle::from_rgba` in the
  decode task), never in `view`.
* **Message media** (`app/media_anim.rs`): GIF / animated WebP attachments
  and embed images, GIF / APNG stickers and animated custom emoji (content
  and reactions) of the newest 30 messages of the open conversation play by
  themselves while "Play GIFs automatically" is on (default, as in the
  official client) and the window is focused. Attachments and stickers are
  decoded from the bytes the image loader already fetched (classes `Media`,
  `Sticker`); emoji fetch their `.gif`. They are drawn by
  `ui::anim_image::AnimImage`, which records the frame time on each redraw and
  calls `Shell::request_redraw_at` for its next frame change while it is
  inside the viewport: no `Message`, no `view()`/layout per frame, and
  tiny-skia repaints only its rectangle. Redraw times are rounded up to a
  60 Hz grid shared by every image, so any number of animations cost at
  most 60 window redraws a second. Off-screen or unfocused images ask
  for nothing (the official client also pauses GIFs while unfocused).
  `FERROCORD_FRAME_STATS=1` logs redraws, `view()` rebuilds, frame changes
  and redraw lateness every 5 s. With the setting off, animated emoji play while their message is hovered and
  GIFs show their first frame; "Reduce motion" stops everything.

### Display name styles

Effects `solid`, `gradient`, `glow` (falls back to gradient), `prism` and
`gummy` are drawn as a solid colour or a per-character gradient; `neon`,
`toon` and `pop` (glow, outline, drop shadow) are drawn as the first colour.
Animation (prism scroll, gummy squash) is not drawn. Fonts other than the
default gg Sans are not bundled: a bold weight of Inter stands in for them.

### Not verified against a live account

* The response key of `GET /user-profile-effects` (`profile_effect_configs`
  is what clients use; a bare array and `configs` are accepted too) and the
  coordinate system of effect layers (the scale-to-card rule is inferred).
* `passthrough=true` for decoration assets not prefixed `a_` (those are
  treated as still).
* Server-specific display name styles and nameplates in member objects.

## 11. Embeds (`app/embeds.rs`, `ui/embeds.rs`)

* **Model.** `Embed` keeps `author.icon_url`/`proxy_icon_url` (the proxy one
  wins), `footer.icon_url`, `timestamp`, `provider.url`, `video`, `flags`;
  `EmbedMedia` keeps both `url` and `proxy_url`. Only the proxy URL is
  downloaded (`EmbedMedia::download_url`), through the same host allowlist as
  every CDN fetch (`cdn.discordapp.com`, `media.discordapp.net`,
  `images-ext-*.discordapp.net`, no token). Messages carry `flags`; bit 2 is
  `SUPPRESS_EMBEDS`. Cached JSON from older versions loads (`#[serde(default)]`).
* **Layout** is decided by pure functions: `plan` (small 80 px thumbnail on the
  right, large image / large `article`/`link` thumbnail (>= 300 px wide) /
  video poster below, 400x300 box, no upscaling, boxes fixed before the
  download so history does not jump), `field_rows` (3 inline fields per row,
  2 beside a thumbnail, non-inline on their own row), `group_embeds` +
  `grid_columns` (embeds sharing a `url` and carrying only an image join the
  lead; 2/3/4 photos as a grid).
* **Video cards** show the poster with a play button. `video_action` routes a
  click: a direct mp4/webm on the media proxy -> `Ui::PlayVideo(url)` (the
  hook for the inline player; today it asks via the confirm-link dialog and
  opens the browser), anything else (YouTube, Twitch iframes) -> the page
  link through `Ui::ConfirmLink`. `gifv` is bare media: its still frame,
  then its MP4 (`gifv_source`) looping muted like a GIF (section 12).
* **Safety.** Only http(s) reaches `pending_link`/the browser; texts are cut
  to Discord's own limits (`clip`), at most 10 embeds and 25 fields are drawn.
* **Live updates.** `MESSAGE_UPDATE` is a `MessagePatch` of `Option`s: an
  embeds-only update (link unfurl) never touches the content, and `embeds: []`
  removes them. The same patch is applied to the SQLite cache.
* **Suppress.** The "X" (author or `MANAGE_MESSAGES`) hides the embeds at once and
  sends `PATCH /channels/{c}/messages/{m}` with `{"flags": flags | 4}`; a
  refusal puts them back. Messages with the flag draw no embeds.
* **Spoilers.** An embed whose URL appears inside `||...||` in the message
  text shows its media behind a click (`Ui::RevealSpoiler`).

Renderer note: tiny-skia ignores image `crop`, container `clip` and corner
radii, so an image can never be "covered" into a box. Posters take the aspect
ratio of their own thumbnail, and grid cells letterbox (`Contain`) on the
card background.

## 12. Video playback (`media/`, `app/video.rs`, `ui/video.rs`, `bin/ferrocord-player`)

**What plays.** Attachments whose `content_type` is `video/*` or whose name
ends in `.mp4 .m4v .mov .webm .mkv`, and embeds (`type: video`, a bare video
link) whose `video.proxy_url` / `video.url` is a file with such an extension
(`media::clip::Clip`). `gifv` / `image` embeds keep their bare-image rendering
(a `gifv` loops instead, see *gifv* below),
and page players (YouTube, ...) stay link cards: their `video.url` is a web
page, never fetched. The inline box is at most 400x300 px (never enlarged),
like the official client.

**Embed cards.** `ui::embeds::video_card` routes through `app::embeds::video_action`: a proxied direct file builds a `Clip` (poster = the card's media box, size from `embed.video`) and plays in the inline player inside the card; iframe-only providers (YouTube, Twitch) keep the confirm-link flow to the browser.

**Poster.** Discord's media proxy renders the first frame of a video as an
image: `{proxy_url}?format=webp&width=W&height=H` (the same URL the official
client uses). Embeds use their `thumbnail` / `image` when present. The poster
goes through the normal bounded image cache. While playing, the poster is
replaced by the live frame.

**Download.** Clicking play downloads the file (`RestClient::download_file`):
only `cdn.discordapp.com` / `media.discordapp.net` (and the configured CDN
override) over HTTPS, no token, the final URL after redirects is checked
again, at most 200 MB, streamed to `<data dir>/videos/ferrocord-video-<pid>-<n>.<ext>`
(`0600`, `create_new`). The file is deleted when the video closes (`Scratch`
is dropped), and files older than 6 h left by a crash are swept at start.
Nothing is downloaded until the user presses play.

**One active video.** `App::video: Option<Playback>`. Starting another video,
leaving the channel, closing the player or logging out drops it: the helper
process ends, and the file is deleted. When a video plays to the end the
helper is released at once (RAM returned); replay starts a new helper on the
file already on disk.

**Helper process.** `ferrocord-player <file>` (installed next to `ferrocord`;
`ferrocord-player.exe` on Windows, spawned with `CREATE_NO_WINDOW`)
demuxes and decodes; the app never links a codec. The container is chosen by
the file's first bytes, not its name.

| | Container | Video (built in) | Audio (built in) |
|---|---|---|---|
| MP4 / MOV (QuickTime) | own ISO-BMFF reader (`ferrocord-player/mp4.rs`): progressive and fragmented (`moof`), `moov` first or last, QuickTime sample descriptions | H.264 8-bit 4:2:0 progressive, Baseline/Main/High incl. B-frames, `avc1`/`avc3` (OpenH264, built from source, static); VP9 (`vp09`) | AAC-LC (symphonia, pure Rust) |
| WebM / MKV | `matroska-demuxer` | VP8 (`vp8-decoder`), VP9 profile 0/1 8-bit (`rusty_vp9`), both pure Rust; H.264 (`V_MPEG4/ISO/AVC`, OBS) | Opus mono/stereo (libopus, static) |

**Platform decoders** (`ferrocord-player/platform/`). What the built-in
decoders cannot handle goes to what the operating system provides, the way
Chromium plays it; nothing is bundled or linked at build time:

| | Linux: system FFmpeg (`platform/ffmpeg.rs`) | Windows: Media Foundation (`platform/mf.rs`) |
|---|---|---|
| Loaded | `dlopen` of `libavcodec.so.62` / `61` / `60` / `59` / `58` + the matching `libavutil` (FFmpeg 8 down to 4) | `mfplat.dll` loaded at run time (Windows N without the Media Feature Pack still starts the helper); sync software MFTs via `MFTEnumEx` |
| H.265 / HEVC (`hvc1`, `hev1`, `dvh1`, `V_MPEGH/ISO/HEVC`), 8 and 10 bit | `hevc` (Fedora's `libavcodec-free` has none: the message says to install `ffmpeg-libs` from RPM Fusion) | "HEVC Video Extensions (from Device Manufacturer)" |
| AV1 (`av01`, `V_AV1`) | `libdav1d`, else `libaom-av1` (FFmpeg's own `av1` is hardware-only) | "AV1 Video Extension" |
| H.264 High 10 / 4:2:2 / 4:4:4 / interlaced, streams with no IDR | `h264` | Microsoft H.264 decoder (interlaced yes; High 10 / 4:4:4 no) |
| AAC in Matroska (`A_AAC`) | `aac` (raw frames with ADTS headers built from the config) | Microsoft AAC decoder (config in `MF_MT_USER_DATA`) |
| Output | `yuv4xxp`, `nv12`, 9-16 bit planar, `p010` → RGBA (`yuv::raw_to_rgba`) | NV12 / P010 / I420 / YV12 → RGBA |

The FFI is the decode API whose signatures never changed (find decoder by
name, alloc / open / send packet / receive frame / flush / free, AVOptions
for `threads` = 4, `ch_layout` / `ac`, `ar`) plus the only struct fields
that never moved (`AVPacket` up to `size`, `AVFrame` up to `format`); pixel
and sample formats are compared by name, FFmpeg logging is silenced.

**Choice.** OpenH264, VP8 and VP9 stay the default for what they handle, so
nothing that played before changes. `codec::new_decoder` sends H.265, AV1
and H.264 with a refused SPS (container header) straight to the platform
decoder; at run time (`Video::switch`) the built-in decoder hands the
stream over, restarting from the first packet, when it refuses an in-band
SPS, shows no picture after 48 packets with a reason (no IDR) or by the end
of the stream, or fails 90 packets in a row before any picture.
`ferrocord-player --backends` prints what this system has
(`platform ffmpeg libavcodec.so.61 61.19.101`, then `h264 h264`,
`hevc -`, ...). `FERROCORD_PLATFORM_DECODERS=0` turns the platform decoders
off, `FERROCORD_VIDEO_DECODER=platform` prefers them for everything
(troubleshooting, tests).

Anything no decoder takes ends with `Failed { missing: true, text }` where
`text` is the codec and, on a second line, what to install: the player
shows "uses <codec>, which the built-in player cannot decode. Install the
FFmpeg libraries (package libavcodec61, ...) to play it here. Or choose
**Open externally** ..." (the only way an external player starts). The
install line names the distribution's package (Debian/Ubuntu
`libavcodec61` / `libavcodec60`, Fedora `ffmpeg-libs` from RPM Fusion, Arch
`ffmpeg`, openSUSE Packman), or on Windows the Microsoft Store extension.
ProRes, MPEG-4 Part 2, encrypted video, 10-bit VP9 without a platform
decoder and Vorbis-only files are named without one. The H.264 variants
OpenH264 refuses (more than 8 bits per sample, 4:2:2 / 4:4:4, interlaced
`frame_mbs_only_flag = 0`) are read from the SPS (`h264.rs`, header or in
band) so the message names them instead of "no picture could be decoded".
OpenH264 shows nothing until an IDR picture; its error concealment does not
help, so it stays off and the platform decoder takes such streams. A video track with unsupported audio
plays silently. Sound goes out through `cpal`; the audio the device consumed
drives the clock (picture follows sound), the wall clock takes over without a
device or when the device stalls. Rotation (`tkhd` matrix) is applied.
Pictures are scaled to at most 960x540 in the helper (nearest), so a 4K video
costs the same pipe and RAM as 540p. Frames are sent at most every 30 ms.
Picture sizes above 8192 per side are refused.

**Wire format** (`media/video_proto.rs`). Helper to app: tag byte + little
endian fields: `R` ready (w, h, duration ms, 0 = unknown), `F` frame (w, h,
RGBA), `T` time (position, duration, state), `E` failed (kind, length, UTF-8,
at most 2 KiB). The reader rejects unknown tags, frame sizes above 4096 per
side / 32 MB and oversized text before allocating. App to helper: lines
`play`, `pause`, `seek <ms>`, `volume <0-100>`, `mute <0|1>`, `quit`;
anything else is ignored. stdin closing (the app died) ends the helper. A
helper that exits without a message is reported with the dynamic loader's
last stderr line (missing library) or "the helper exited".

**Frames.** Each `Frame` becomes an `image::Handle` in `App::update` (new id
per frame, created once, never in `view`). The event stream keeps only the
newest frame when the UI falls behind.

**Controls.** Play/pause (click on the picture too), time, seek bar (drag
holds the thumb, the seek is sent on release), mute, volume, full screen. Full
screen is a layer in the window stack that also switches the window to
fullscreen (not in mock mode); Esc or the minimise button leaves it and the
video keeps playing inline. Controls are always laid out: nothing changes
size on hover.

**Mock mode.** `--mock` plays a scripted test pattern (`media::video::spawn_mock`)
instead of downloading: no network, no disk, no helper. Screenshots:
`--screenshot video` and `--screenshot video-fullscreen`.

**gifv.** Tenor / Giphy links arrive as `gifv` embeds whose `video` is a
short silent MP4; like the official client it loops muted, with no controls,
and several can play at once. It does not use the viewer above:
`app::media_anim` treats the MP4 (`gifv_source`) as one more animated
message asset (class `Gifv`) of the newest 30 messages, at most 6 clips. The
file is downloaded (16 MB cap, allowlisted hosts) to a scratch file, and the
helper runs once in *frames mode* (`ferrocord-player --frames <w>x<h>
<frames> <bytes> <file>`, `media::gifv`): picture track only, decoded as fast
as possible, fitted to 400x300, evenly spaced frames within 48 frames / 6 MB
(`gifv::Sampler`, at most ~30 fps and the first 30 s), each written as a `P`
message (time + RGBA), then it exits and the file is deleted. Delays come
from the frame times; the frames become an `Anim` drawn by
`ui::anim_image`, so a gifv follows the GIF rules ("Play GIFs
automatically", window focus, "Reduce motion", redraws only while visible,
no `view()` per frame). At most two helpers decode at once, each is killed
after 20 s. Without the helper (non-Unix builds) or on any failure the still
frame stays (retried after 5 min). Mock: `mock::MOCK_GIFV` in #showcase.

## 13. Voice and calls

Design, DAVE and the video feasibility pass: `docs/voice.md`.

| Step | Wire |
|---|---|
| Join / move / leave | Gateway op 4 `{guild_id\|null, channel_id\|null, self_mute, self_deaf, self_video: false}` (`GatewayHandle::set_voice`; re-sent after a new session) |
| Session | `VOICE_STATE_UPDATE` (ours: `session_id`) + `VOICE_SERVER_UPDATE` `{token, guild_id \| channel_id, endpoint}` |
| Voice gateway | `wss://{endpoint}/?v=8`: op 0 IDENTIFY (`max_dave_protocol_version: 1`), 8 HELLO, 2 READY, 1 SELECT PROTOCOL, 4 SESSION DESCRIPTION, 3/6 heartbeat `{t, seq_ack}`, 5 SPEAKING, 7/9 RESUME, 11/13 clients connect/disconnect, 21-31 DAVE (binary frames `seq u16, op u8, payload`) |
| UDP | IP discovery (type 1/2, length 70, SSRC, 64-byte address, port); RTP PT 120 Opus 48 kHz stereo; `aead_aes256_gcm_rtpsize` preferred, `aead_xchacha20_poly1305_rtpsize` otherwise; 32-bit nonce counter appended; header (+CSRCs, extension preamble) is the AAD |
| Close codes | 4014 / 4022 / 4021 / 4004 / 4011 / 4016 / 4017 end the call; 4006 / 4009 rejoin through op 4; others resume (5 tries) |
| Calls | `CALL_CREATE` / `CALL_UPDATE` `{channel_id, message_id, ringing, voice_states}`, `CALL_DELETE`; `POST /channels/{id}/call/ring {recipients: null}` when starting a call; `POST /channels/{id}/call/stop-ringing {recipients: [me]}` to decline |
| History | Message type 3 with `call {participants, ended_timestamp}` |
| READY | `guilds[].voice_states` (and `GUILD_CREATE`) fill the users under voice channels |

Voice settings (devices, sensitivity, per-user volume, self mute/deafen)
are stored in SQLite `meta("pref.voice")` as JSON and kept on logout.

## 14. User Settings (`net/user_settings.rs`, `net/rest/settings.rs`, `app/settings.rs`, `ui/settings.rs`)

The screen copies the official layout: a sidebar of categories (User
Settings, Activity Settings, App Settings, Log Out at the bottom) and the
page on the right; Esc or the round X closes it. Both scrollables have
fixed ids, text fields are bound to `SettingsState::forms` (filled once when
the screen opens) and no image handle is made in `view`.

### Synced settings: the preloaded user settings proto

`GET /users/@me/settings-proto/1` -> `{settings: base64}` =
`PreloadedUserSettings` (discord-protos 1.2.429, checked against the PyPI
package's `.proto`). Ferrocord identifies with `capabilities: 0`, so READY
carries only the legacy JSON settings: the proto is read with one GET after
READY and again whenever the settings screen opens (so changes made in
another client show).

| Setting | Field (top.field[.field]) | Type |
|---|---|---|
| Show spoiler content | 6.4 `render_spoilers` | StringValue `ON_CLICK` / `IF_MODERATOR` / `ALWAYS` |
| Images when uploaded directly | 6.9 `inline_attachment_media` | BoolValue |
| Images when posted as links | 6.10 `inline_embed_media` | BoolValue |
| Play GIFs automatically | 6.11 `gif_auto_play` | BoolValue |
| Show embeds / link previews | 6.12 `render_embeds` | BoolValue |
| Show emoji reactions | 6.13 `render_reactions` | BoolValue |
| Play animated emoji | 6.14 `animate_emoji` | BoolValue |
| Animate stickers | 6.15 `animate_stickers` | UInt32Value 0 always, 1 on interaction, 2 never |
| Compact message display | 6.17 `message_display_compact` | BoolValue |
| Convert emoticons | 6.21 `convert_emoticons` | BoolValue |
| DM spam filter | 6.27 `dm_spam_filter_v2` | enum 1 off, 2 non-friends, 3 all |
| Allow DMs from server members (default) | 8.4 `default_guilds_restricted` (inverted) | bool |
| Friend request sources | 8.11 `friend_source_flags` | UInt32Value bits 1<<1 mutual friends, 1<<2 mutual servers, 1<<3 everyone (needs both) |
| Online status | 11.1 `status.status` | StringValue |
| Custom status | 11.2 `custom_status {text=1, emoji_id=2 fixed64, emoji_name=3, expires_at_ms=4 fixed64, created_at_ms=5 fixed64}` | message |
| Language | 12.1 `localization.locale` | StringValue |
| Theme | 13.1 `appearance.theme` | enum 1 dark, 2 light, 3 darker (Ash), 4 midnight (Onyx) |
| Developer Mode | 13.2 `developer_mode` | bool |
| Time format | 13.9 `timestamp_hour_cycle` | enum 0 auto, 1 12-hour, 2 24-hour |

A `*Value` wrapper that is absent means "not set" (the client default); an
empty wrapper is the zero value. Writes follow Discord's PATCH rule: each
top-level field sent *replaces* the stored one and omitted ones stay. So a
save is GET, edit (`user_settings::apply`: every other byte, unknown fields
included, kept), then `PATCH /users/@me/settings-proto/1 {settings: base64}`
with only the touched top-level fields, each complete
(`user_settings::top_level`). `out_of_date: true` counts as a failure.

Changes apply at once and are queued; the queue is written after 1.5 s of
quiet (one PATCH per batch, the endpoint is heavily rate limited) or when
the screen closes. Saves are serialised. A failed save restores what the
batch changed (unless a newer change of the same setting is queued) and
shows a toast. The status menu and the tray use the same path; the gateway
presence (op 3) changes immediately.

### Profile

Display name: `PATCH /users/@me {global_name}` (empty = `null`). Pronouns and
About Me: `PATCH /users/@me/profile {pronouns, bio}`. Only changed fields
are sent, from a Save Changes / Reset bar like the official one. The
display name shows optimistically and is put back if Discord refuses (its
400 message is shown).

### Local settings

`meta("pref.settings")` JSON (`LocalSettings`, clamped on load): chat font
size (12-24 px; message text and author names), zoom (50-150 %; the
daemon's `scale_factor`), saturation (palette desaturated in
`Palette::saturated`), notification sound (freedesktop `sound-name:
message-new-instant` or `suppress-sound`; Windows default toast audio or
silent), unread badge (tray count), taskbar flash
(`window::request_user_attention` on a mention while unfocused) and message
content in notifications. The existing `pref.*` flags (minimize to tray,
start minimized, mute notifications, reduce motion) stay where they were;
`pref.autoplay_gifs` now mirrors the synced `gif_auto_play` until Discord
answers. Voice & Video adds input/output volume (0-200 %, applied in the
engine before voice activity / after mixing) and a microphone test
(`net/voice/mic_test.rs`: opens the devices outside a call, drives the
meter and plays the voice back).

### Not implemented

Email / phone / password / 2FA / username changes, account disable/delete
(managed on discord.com); avatar, banner, profile theme and effects
editing; explicit image filters (Ferrocord does not blur flagged media, so
the switch would do nothing here); sticker suggestions (no suggestion UI);
push to talk (no global shortcut); per-type notification switches beyond
the ones above (reaction, friend and activity notifications are not
produced); keybind editing; translating Ferrocord's own UI (English only;
the Language page sets the account locale); `USER_SETTINGS_PROTO_UPDATE`
is not handled (the screen re-reads the proto when it opens instead).

## 15. Permissions and server administration (`app/permissions.rs`, `app/guild_admin.rs`, `app/channel_admin.rs`, `app/role_admin.rs`, `net/rest/guild.rs`, `ui/server_settings.rs`, `ui/channel_admin.rs`, `ui/roles.rs`)

**Permissions.** Computed like the official client: the owner and
Administrator get everything; otherwise the server-wide bits are the union of
`@everyone` (the role whose id is the guild id) and the user's roles (READY
`merged_members`, `GUILD_MEMBER_UPDATE`). In a channel the overwrites
(`permission_overwrites`: `type` 0 role / 1 member, `allow`/`deny` decimal
strings) apply in order: `@everyone`, then all of the user's roles together
(denies, then allows), then the member overwrite. Threads use their parent's.
Role changes arrive as `GUILD_ROLE_CREATE/UPDATE/DELETE` (and with the whole
guild in `GUILD_UPDATE`). Channels without View Channel are left out of the
channel list and the unread badges; a category shows while it holds a visible
channel or is visible itself. A server whose roles lack `@everyone` (partial
data) is shown unfiltered.

**Server dropdown.** The chevron in the channel-list header opens the same
menu as the rail's right-click, with the admin actions first:

| Action | Shown when | Call |
|---|---|---|
| Invite People | Create Invite + View Channel in some channel (the open one first) | `POST /channels/{id}/invites {max_age: 604800, max_uses: 0, temporary: false}` -> `code`; the dialog shows `https://discord.gg/{code}` with Copy. |
| Server Settings | Manage Server or Manage Roles | opens the Server Settings screen (Overview, or Roles without Manage Server). |
| Leave Server | not the owner | confirmation, then `DELETE /users/@me/guilds/{id} {lurking: false}`. |

**Server Settings** (full window, Esc closes). Overview: name (2-100
characters) and icon (PNG/JPEG/GIF/WebP `data:` URI, 4 MB, or removed).
"Save Changes" sends only the changed fields with `PATCH /guilds/{id}
{name?, icon?}`; the new name shows at once, the icon when `GUILD_UPDATE`
arrives. Delete Server (owner only) asks for the server name typed exactly,
then `DELETE /guilds/{id}`. Leaving or deleting closes everything about the
server and goes Home; the gateway's `GUILD_DELETE` removes it. A 403 reads
"You do not have permission"; a two-factor requirement points to the official
client (Ferrocord has no MFA prompt).

**Channels** (`app/channel_admin.rs`, `ui/channel_admin.rs`), for users with
Manage Channels (server-wide to create; in the channel to edit, move or
delete):

| Action | Where | Call |
|---|---|---|
| Create Channel / Category | server dropdown; "Create Channel" in a category's right-click menu and its "+" | `POST /guilds/{id}/channels {name, type: 0 text / 2 voice / 4 category, parent_id?, permission_overwrites: []}`. Text names are typed like the official client (lower case, spaces as dashes). The new text channel opens. |
| Edit Channel / Category | channel or category right-click menu | `PATCH /channels/{id}` with the changed fields only: `name`, `topic` (`null` clears), `nsfw`, `parent_id` (`null` = no category). Applied locally on success; `CHANNEL_UPDATE` confirms. |
| Move Up / Move Down | same menu | siblings (all categories, or the channels under the same category in the same text/voice group, by `(position, id)`) are renumbered 0.. in the new order and the changed ones sent as `PATCH /guilds/{id}/channels [{id, position}]`. Applied at once; a failure restores the old positions. |
| Delete | same menu, or the Edit dialog | confirmation, then `DELETE /channels/{id}`. Removed locally at once (an open channel switches to the server's first one); deleting a category keeps its channels, uncategorised. |

**Roles** (`app/role_admin.rs`, `ui/roles.rs`; Server Settings -> Roles, for
users with Manage Roles). Hierarchy like the official client: the owner is
above every role, everyone else ranks at their highest role's `position`;
a role can be edited, deleted or given only when it is below the user's top
role, and only permissions the user has can be switched on (Administrator
can grant anything). `@everyone` is always listed and edits only its
permissions.

| Action | Call |
|---|---|
| Create Role | `POST /guilds/{id}/roles {name: "new role", permissions: "0"}`; the new role is selected. |
| Save Changes (name, colour, permission switches) | `PATCH /guilds/{id}/roles/{role}` with the changed fields only: `name`, `color`, `permissions` (decimal string); applied locally, `GUILD_ROLE_UPDATE` confirms. |
| Delete Role | confirmation, then `DELETE /guilds/{id}/roles/{role}`. |
| Member's Roles (user right-click -> Roles, in a server) | `PUT` / `DELETE /guilds/{id}/members/{user}/roles/{role}`; applied at once, rolled back on failure. |

**Member moderation** (user right-click in a server). Each item shows only
with the permission and a higher rank than the member (never on oneself or
the owner). The optional reason goes in `X-Audit-Log-Reason`
(percent-encoded, 512 chars).

| Action | Permission | Call |
|---|---|---|
| Timeout (60 s, 5 min, 10 min, 1 h, 1 day, 1 week) | Timeout Members | `PATCH /guilds/{id}/members/{user} {communication_disabled_until: ISO-8601}`; "Remove Timeout" sends `null`. |
| Kick | Kick Members | `DELETE /guilds/{id}/members/{user}`. |
| Ban (delete none / 1 h / 24 h / 7 days of messages) | Ban Members | `PUT /guilds/{id}/bans/{user} {delete_message_seconds}`. |

Not implemented yet: channel permission overwrites editing, role
reordering, hoist/mentionable switches, drag-and-drop channel reordering,
unban list, emoji/sticker uploads, audit log, ownership transfer.

