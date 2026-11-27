//! App glue for plugins (`crate::plugins`): the Plugins page, the hook
//! points in sending / channel switches / gateway events, and the requests
//! plugins hand back (performed here with the app's own code paths).
//!
//! Every hook point is a no-op while no plugin is running.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use iced::{Subscription, Task};

use super::{App, Done, Message, View};
use crate::net::Event;
use crate::net::model::*;
use crate::plugins::hostcalls::{self, Grant, Out};
use crate::plugins::manifest::Manifest;
use crate::plugins::package::{self, Scan};
use crate::plugins::state::State;
use crate::plugins::worker::{Job, Source};
use crate::plugins::{self, abi, message_info};

/// How long a send waits for the transform chain before going out as typed.
pub const TRANSFORM_TIMEOUT: Duration = Duration::from_millis(300);
/// Largest HTTP answer handed to a plugin.
pub const MAX_HTTP_RESPONSE: usize = 1024 * 1024;
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// Most messages of a window sent in one `history` event.
const HISTORY_EVENT_MAX: usize = 50;

/// Plugin messages (page input, worker output, task results).
#[derive(Clone)]
pub enum PluginMsg {
    /// Directory scan done; `boot`: start the enabled plugins; `reload`:
    /// restart them all from disk.
    Scanned {
        state: Option<Box<State>>,
        scan: Box<Scan>,
        boot: bool,
        reload: bool,
    },
    Out(Box<Out>),
    /// The Plugins settings page opened: rescan the folder.
    OpenPage,
    /// The switch next to a plugin.
    Toggle(String),
    ConfirmEnable,
    CancelPrompt,
    SetBool(String, String, bool),
    SetChoice(String, String, String),
    EditText(String, String, String),
    Reload,
    OpenFolder,
    /// A message context-menu item of a plugin.
    Action {
        plugin: String,
        action: String,
        message: MessageId,
    },
    TransformTimeout(u64),
    HttpDone {
        plugin: String,
        request: u64,
        grant: Option<Grant>,
        response: Box<abi::HttpResponse>,
    },
}

impl std::fmt::Debug for PluginMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Out(o) => write!(f, "PluginMsg::Out({o:?})"),
            Self::Scanned { .. } => f.write_str("PluginMsg::Scanned"),
            Self::Toggle(id) => write!(f, "PluginMsg::Toggle({id})"),
            Self::Action { plugin, action, .. } => {
                write!(f, "PluginMsg::Action({plugin}/{action})")
            }
            Self::TransformTimeout(t) => write!(f, "PluginMsg::TransformTimeout({t})"),
            Self::HttpDone { plugin, .. } => write!(f, "PluginMsg::HttpDone({plugin})"),
            other => f.write_str(match other {
                Self::OpenPage => "PluginMsg::OpenPage",
                Self::ConfirmEnable => "PluginMsg::ConfirmEnable",
                Self::CancelPrompt => "PluginMsg::CancelPrompt",
                Self::Reload => "PluginMsg::Reload",
                Self::OpenFolder => "PluginMsg::OpenFolder",
                _ => "PluginMsg::Setting",
            }),
        }
    }
}

fn msg(m: PluginMsg) -> Message {
    Message::Plugin(m)
}

/// Subscription key: one stream per plugin-thread generation.
struct OutKey {
    generation: u64,
    rx: Arc<Mutex<Option<futures::channel::mpsc::Receiver<Out>>>>,
}

impl std::hash::Hash for OutKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        "plugins".hash(state);
        self.generation.hash(state);
    }
}

fn out_stream(k: &OutKey) -> futures::stream::BoxStream<'static, Out> {
    match k.rx.lock().ok().and_then(|mut r| r.take()) {
        Some(rx) => rx.boxed(),
        None => futures::stream::empty().boxed(),
    }
}

fn scan_task(dir: std::path::PathBuf, boot: bool) -> Task<Message> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                let state = boot.then(|| Box::new(State::load(&dir)));
                // At boot nothing is scanned unless something is enabled.
                let scan = if state.as_ref().is_none_or(|s| s.any_enabled()) {
                    package::scan(&dir)
                } else {
                    Scan::default()
                };
                (state, Box::new(scan))
            })
            .await
            .unwrap_or_default()
        },
        move |(state, scan)| {
            msg(PluginMsg::Scanned {
                state,
                scan,
                boot,
                reload: false,
            })
        },
    )
}

impl App {
    /// Startup: nothing at all unless a state file exists (then its
    /// enabled plugins are started off the UI thread).
    pub(super) fn boot_plugins(&mut self) -> Task<Message> {
        match &self.plugins.dir {
            Some(dir) if self.plugins.may_have_enabled() => scan_task(dir.clone(), true),
            _ => Task::none(),
        }
    }

    /// The plugin thread's outputs, while it runs.
    pub(super) fn plugin_subscription(&self) -> Option<Subscription<Message>> {
        let rx = self.plugins.receiver()?;
        let key = OutKey {
            generation: self.plugins.running_generation()?,
            rx,
        };
        Some(Subscription::run_with(key, out_stream).map(|o| msg(PluginMsg::Out(Box::new(o)))))
    }

    fn persist_plugins(&self) -> Task<Message> {
        let (Some(dir), true) = (self.plugins.dir.clone(), self.plugins.persist) else {
            return Task::none();
        };
        let state = self.plugins.state.clone();
        Task::perform(
            async move { tokio::task::spawn_blocking(move || state.save(&dir)).await },
            |r| {
                if !matches!(r, Ok(Ok(()))) {
                    tracing::warn!("could not save the plugins state");
                }
                Message::Done(Done::Ignored)
            },
        )
    }

    fn plugin_name(&self, id: &str) -> String {
        self.plugins
            .manifest(id)
            .map(|m| m.name.clone())
            .or_else(|| {
                self.plugins
                    .scan
                    .as_ref()
                    .and_then(|s| s.get(id))
                    .map(|f| f.manifest.name.clone())
            })
            .unwrap_or_else(|| id.to_owned())
    }

    /// Starts `m` from the scanned folder (the grant must cover it).
    fn start_plugin(&mut self, m: &Manifest, path: std::path::PathBuf) {
        self.plugins.load(m, Source::Path(path));
    }

    /// Disables `id` (persisted) and unloads it.
    fn disable_plugin(&mut self, id: &str, reason: Option<String>) -> Task<Message> {
        self.plugins.state.disable(id, reason);
        self.plugins.unload(id);
        self.persist_plugins()
    }

    pub(super) fn on_plugin(&mut self, m: PluginMsg) -> Task<Message> {
        match m {
            PluginMsg::Scanned {
                state,
                scan,
                boot,
                reload,
            } => {
                if let Some(s) = state {
                    self.plugins.state = *s;
                }
                self.plugins.scan = Some(*scan);
                self.plugins.page.scanning = false;
                self.fill_plugin_drafts();
                if boot || reload {
                    return self.start_enabled(reload);
                }
                Task::none()
            }
            PluginMsg::Out(o) => self.on_plugin_out(*o),
            PluginMsg::OpenPage => {
                self.plugins.page.prompt = None;
                self.rescan()
            }
            PluginMsg::Toggle(id) => {
                if self.plugins.state.is_enabled(&id) {
                    self.disable_plugin(&id, None)
                } else {
                    // Enabling always shows what the plugin may do first.
                    self.plugins.page.prompt = Some(id);
                    Task::none()
                }
            }
            PluginMsg::CancelPrompt => {
                self.plugins.page.prompt = None;
                Task::none()
            }
            PluginMsg::ConfirmEnable => {
                let Some(id) = self.plugins.page.prompt.take() else {
                    return Task::none();
                };
                let Some(found) = self.plugins.scan.as_ref().and_then(|s| s.get(&id)).cloned()
                else {
                    return Task::none();
                };
                self.plugins.state.enable(&found.manifest);
                self.start_plugin(&found.manifest, found.path);
                self.persist_plugins()
            }
            PluginMsg::SetBool(id, key, v) => {
                self.set_plugin_setting(&id, &key, abi::SettingValue::Bool(v))
            }
            PluginMsg::SetChoice(id, key, v) => {
                self.set_plugin_setting(&id, &key, abi::SettingValue::Str(v))
            }
            PluginMsg::EditText(id, key, text) => {
                let spec = self
                    .plugins
                    .scan
                    .as_ref()
                    .and_then(|s| s.get(&id))
                    .and_then(|f| f.manifest.setting(&key))
                    .cloned();
                self.plugins
                    .page
                    .drafts
                    .insert((id.clone(), key.clone()), text.clone());
                let value = match spec.map(|s| s.kind) {
                    Some(plugins::manifest::SettingKind::Int { .. }) => match text.trim().parse() {
                        Ok(i) => abi::SettingValue::Int(i),
                        Err(_) => return Task::none(),
                    },
                    _ => abi::SettingValue::Str(text),
                };
                self.set_plugin_setting(&id, &key, value)
            }
            PluginMsg::Reload => {
                self.plugins.page.prompt = None;
                let Some(dir) = self.plugins.dir.clone() else {
                    return Task::none();
                };
                self.plugins.page.scanning = true;
                // Rescan, then restart every enabled plugin from disk.
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || package::scan(&dir))
                            .await
                            .unwrap_or_default()
                    },
                    |scan| {
                        msg(PluginMsg::Scanned {
                            state: None,
                            scan: Box::new(scan),
                            boot: false,
                            reload: true,
                        })
                    },
                )
            }
            PluginMsg::OpenFolder => {
                let Some(dir) = self.plugins.dir.clone() else {
                    return Task::none();
                };
                if self.cfg.mock {
                    return Task::none();
                }
                Task::perform(
                    async move {
                        let d = dir.clone();
                        let _ =
                            tokio::task::spawn_blocking(move || std::fs::create_dir_all(d)).await;
                        super::opener::open(dir.to_string_lossy().into_owned()).await
                    },
                    |_| Message::Done(Done::Ignored),
                )
            }
            PluginMsg::Action {
                plugin,
                action,
                message,
            } => {
                self.menu = None;
                let Some(c) = self.open_chan() else {
                    return Task::none();
                };
                let Some(m) = self.chat.message(c, message) else {
                    return Task::none();
                };
                let info = message_info(m, self.chat.me_id());
                let guild = m.guild_id.map(|g| g.0);
                let grant = self.plugins.grant(c.0, guild, Some(message.0));
                self.plugins.send(Job::Action {
                    id: plugin,
                    input: abi::ActionInput {
                        action,
                        message: info,
                    },
                    grant,
                });
                Task::none()
            }
            PluginMsg::TransformTimeout(ticket) => match self.plugins.pending_transform.take() {
                Some(p) if p.ticket == ticket => {
                    tracing::warn!("plugin transform timed out; sending as typed");
                    self.send_plugin_text(Snowflake(p.channel), p.content, true)
                }
                other => {
                    self.plugins.pending_transform = other;
                    Task::none()
                }
            },
            PluginMsg::HttpDone {
                plugin,
                request,
                grant,
                mut response,
            } => {
                if let Some(n) = self.plugins.http_inflight.get_mut(&plugin) {
                    *n = n.saturating_sub(1);
                }
                response.request_id = request;
                self.plugins.send(Job::HttpDone {
                    id: plugin,
                    response: *response,
                    grant,
                });
                Task::none()
            }
        }
    }

    /// Text and number settings are edited in `page.drafts`, filled once
    /// per scan (when the page opens), never rebuilt from the state while
    /// the user types.
    fn fill_plugin_drafts(&mut self) {
        let Some(scan) = &self.plugins.scan else {
            return;
        };
        let mut drafts = std::collections::HashMap::new();
        for f in &scan.found {
            let stored = self
                .plugins
                .state
                .plugins
                .get(&f.manifest.id)
                .map(|r| r.settings.clone())
                .unwrap_or_default();
            for (key, v) in f.manifest.effective_settings(&stored) {
                let text = match v {
                    abi::SettingValue::Int(i) => i.to_string(),
                    abi::SettingValue::Str(s) => s,
                    abi::SettingValue::Bool(_) => continue,
                };
                drafts.insert((f.manifest.id.clone(), key), text);
            }
        }
        self.plugins.page.drafts = drafts;
    }

    fn rescan(&mut self) -> Task<Message> {
        match self.plugins.dir.clone() {
            Some(dir) => {
                self.plugins.page.scanning = true;
                scan_task(dir, false)
            }
            None => Task::none(),
        }
    }

    /// Starts every enabled plugin whose grant still covers its manifest;
    /// `reload` restarts the running ones too (hot reload).
    fn start_enabled(&mut self, reload: bool) -> Task<Message> {
        let Some(scan) = self.plugins.scan.clone() else {
            return Task::none();
        };
        let mut changed = false;
        let enabled: Vec<String> = self
            .plugins
            .state
            .plugins
            .iter()
            .filter(|(_, r)| r.enabled)
            .map(|(id, _)| id.clone())
            .collect();
        for id in enabled {
            let Some(found) = scan.get(&id) else {
                // Removed from disk: stop it, keep it enabled for when it
                // comes back.
                self.plugins.unload(&id);
                continue;
            };
            let covered = self
                .plugins
                .state
                .plugins
                .get(&id)
                .is_some_and(|r| r.covers(&found.manifest));
            if !covered {
                self.plugins.state.disable(
                    &id,
                    Some("The new version asks for more permissions: enable it again to review them.".into()),
                );
                self.plugins.unload(&id);
                changed = true;
                continue;
            }
            if reload || self.plugins.manifest(&id).is_none() {
                self.start_plugin(&found.manifest, found.path.clone());
            }
        }
        if changed {
            self.persist_plugins()
        } else {
            Task::none()
        }
    }

    fn set_plugin_setting(&mut self, id: &str, key: &str, v: abi::SettingValue) -> Task<Message> {
        let Some(spec) = self
            .plugins
            .scan
            .as_ref()
            .and_then(|s| s.get(id))
            .and_then(|f| f.manifest.setting(key))
        else {
            return Task::none();
        };
        if spec.check(&v).is_err() {
            return Task::none();
        }
        self.plugins
            .state
            .plugins
            .entry(id.to_owned())
            .or_default()
            .settings
            .insert(key.to_owned(), v.clone());
        self.plugins.send(Job::SettingChanged {
            id: id.to_owned(),
            key: key.to_owned(),
            value: v,
        });
        self.persist_plugins()
    }

    fn on_plugin_out(&mut self, o: Out) -> Task<Message> {
        match o {
            Out::Loaded { id, error } => {
                let ok = error.is_none();
                if self.plugins.loaded(&id, ok).is_none() {
                    // Disabled while it was loading.
                    return Task::none();
                }
                match error {
                    Some(e) => {
                        self.toast = Some(format!(
                            "Plugin \u{201c}{}\u{201d} could not start: {e}",
                            self.plugin_name(&id)
                        ));
                        self.disable_plugin(&id, Some(e))
                    }
                    None => {
                        // Let it see the open channel right away.
                        self.plugin_channel_switch();
                        Task::none()
                    }
                }
            }
            Out::Faulted {
                id,
                reason,
                disabled,
            } => {
                if !disabled {
                    return Task::none();
                }
                let name = self.plugin_name(&id);
                self.plugins.faulted_out(&id);
                self.toast = Some(format!(
                    "Plugin \u{201c}{name}\u{201d} was turned off: it {reason} repeatedly."
                ));
                self.plugins.state.disable(
                    &id,
                    Some(format!("Turned off after repeated errors: it {reason}.")),
                );
                self.persist_plugins()
            }
            Out::Toast { id, text } => {
                if self.plugins.manifest(&id).is_some() {
                    self.toast = Some(format!("{}: {text}", self.plugin_name(&id)));
                }
                Task::none()
            }
            Out::Transformed { ticket, text } => match self.plugins.pending_transform.take() {
                Some(p) if p.ticket == ticket => {
                    let content = text.unwrap_or(p.content);
                    self.send_plugin_text(Snowflake(p.channel), content, true)
                }
                other => {
                    self.plugins.pending_transform = other;
                    Task::none()
                }
            },
            Out::Send {
                id,
                channel,
                content,
            } => {
                if self.plugins.manifest(&id).is_none()
                    || !self.chat.channels.contains_key(&Snowflake(channel))
                {
                    return Task::none();
                }
                tracing::info!(plugin = %id, "plugin sends a message for a user action");
                self.send_plugin_text(Snowflake(channel), content, false)
            }
            Out::React {
                id,
                channel,
                message,
                emoji,
            } => {
                let (c, m) = (Snowflake(channel), Snowflake(message));
                let e = Emoji::unicode(&emoji);
                let already = self
                    .chat
                    .message(c, m)
                    .is_some_and(|msg| msg.reactions.iter().any(|r| r.emoji == e && r.me));
                if self.plugins.manifest(&id).is_none() || already {
                    return Task::none();
                }
                self.react(c, m, e)
            }
            Out::Clipboard { id, text } => {
                if self.plugins.manifest(&id).is_none() {
                    return Task::none();
                }
                iced::clipboard::write(text)
            }
            Out::Composer { id, text } => {
                if self.plugins.manifest(&id).is_some() {
                    self.composer.push_str(&text);
                }
                iced::widget::operation::focus(super::actions::composer_id())
            }
            Out::Decorate {
                id,
                message,
                decorations,
            } => {
                if self.plugins.manifest(&id).is_some() {
                    self.plugins.decorations.set(&id, message, decorations);
                }
                Task::none()
            }
            Out::Http {
                id,
                request,
                req,
                grant,
            } => self.plugin_http(id, request, req, grant),
            Out::SettingSaved { id, key, value } => {
                self.plugins
                    .state
                    .plugins
                    .entry(id)
                    .or_default()
                    .settings
                    .insert(key, value);
                self.persist_plugins()
            }
        }
    }

    /// Sends `content` with the app's own send path. `with_reply`: keep
    /// the reply being composed (the user's own message); plugin sends
    /// never take it.
    fn send_plugin_text(
        &mut self,
        c: ChannelId,
        content: String,
        with_reply: bool,
    ) -> Task<Message> {
        if with_reply {
            return self.send_text(c, content);
        }
        let reply = self.reply.take();
        let t = self.send_text(c, content);
        self.reply = reply;
        t
    }

    fn plugin_http(
        &mut self,
        id: String,
        request: u64,
        mut req: abi::HttpRequest,
        grant: Option<Grant>,
    ) -> Task<Message> {
        let fail = |e: &str| {
            msg(PluginMsg::HttpDone {
                plugin: id.clone(),
                request,
                grant,
                response: Box::new(abi::HttpResponse {
                    error: Some(e.to_owned()),
                    ..Default::default()
                }),
            })
        };
        let Some(m) = self.plugins.manifest(&id).cloned() else {
            return Task::none();
        };
        // Checked again here: the network leaves from this side.
        if hostcalls::check_http(&m, &mut req).is_err() {
            return Task::done(fail("host not allowed"));
        }
        if self.cfg.mock {
            return Task::done(fail("network is off in mock mode"));
        }
        let n = self.plugins.http_inflight.entry(id.clone()).or_default();
        if *n >= plugins::MAX_HTTP_INFLIGHT {
            return Task::done(fail("too many requests"));
        }
        *n += 1;
        let client = match &self.plugins.http {
            Some(c) => c.clone(),
            None => {
                // Its own client: no cookies, no Discord headers, no
                // redirects (a redirect could leave the allowlist).
                let c = reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(HTTP_TIMEOUT)
                    .connect_timeout(Duration::from_secs(5))
                    .https_only(true)
                    .user_agent(concat!("Ferrocord-Plugin/", env!("CARGO_PKG_VERSION")))
                    .build();
                match c {
                    Ok(c) => {
                        self.plugins.http = Some(c.clone());
                        c
                    }
                    Err(_) => return Task::done(fail("no HTTP client")),
                }
            }
        };
        Task::perform(fetch(client, req), move |r| {
            msg(PluginMsg::HttpDone {
                plugin: id.clone(),
                request,
                grant,
                response: Box::new(r),
            })
        })
    }

    /// Screenshot screens: the example plugins as if installed (no wasm
    /// runs; the page only needs their manifests).
    pub(super) fn mock_plugin_screen(&mut self, screen: Option<crate::config::ScreenshotScreen>) {
        use crate::config::ScreenshotScreen as S;
        use crate::plugins::package::Found;
        use crate::plugins::{Decoration, DecorationKind};
        match screen {
            Some(S::Plugins | S::PluginPrompt) => {
                let manifests = [
                    include_str!("../../examples/plugins/text-replacer/plugin.toml"),
                    include_str!("../../examples/plugins/word-highlighter/plugin.toml"),
                    include_str!("../../examples/plugins/quote-message/plugin.toml"),
                ];
                let mut scan = Scan::default();
                for t in manifests {
                    if let Ok(manifest) = Manifest::parse(t) {
                        scan.found.push(Found {
                            path: std::path::PathBuf::from(&manifest.id),
                            manifest,
                        });
                    }
                }
                if let Some(f) = scan.get("word-highlighter") {
                    self.plugins.state.enable(&f.manifest);
                }
                self.plugins.scan = Some(scan);
                self.plugins.dir = Some("~/.config/ferrocord/plugins".into());
                self.fill_plugin_drafts();
                self.settings_open = true;
                self.settings.category = Some(super::settings::Category::Plugins);
                if screen == Some(S::PluginPrompt) {
                    self.plugins.page.prompt = Some("text-replacer".into());
                }
            }
            Some(S::PluginDecorations) => {
                let Some(w) = self.open_channel().and_then(|c| self.chat.messages.get(&c)) else {
                    return;
                };
                let ids: Vec<u64> = w
                    .messages
                    .iter()
                    .rev()
                    .filter(|m| !m.content.is_empty())
                    .take(4)
                    .map(|m| m.id.0)
                    .collect();
                let d = |kind, text: &str, tip: Option<&str>| Decoration {
                    kind,
                    text: text.into(),
                    tooltip: tip.map(Into::into),
                };
                if let [c, b, a, ..] = ids[..] {
                    self.plugins.decorations.set(
                        "word-highlighter",
                        a,
                        vec![d(
                            DecorationKind::Highlight,
                            "rust",
                            Some("Matched by Word Highlighter"),
                        )],
                    );
                    self.plugins.decorations.set(
                        "translator",
                        b,
                        vec![d(
                            DecorationKind::Line,
                            "Translated from Spanish: see you tomorrow at the meetup",
                            None,
                        )],
                    );
                    self.plugins.decorations.set(
                        "read-time",
                        c,
                        vec![d(
                            DecorationKind::Badge,
                            "1 min read",
                            Some("Estimated reading time"),
                        )],
                    );
                }
            }
            _ => {}
        }
    }

    // -- hook points ----------------------------------------------------------

    /// Before a composer send: runs a plugin `/command` or the outgoing
    /// transform. `Some`: the plugins took over (the composer is cleared).
    pub(super) fn plugin_intercept_send(
        &mut self,
        c: ChannelId,
        content: &str,
    ) -> Option<Task<Message>> {
        if !self.plugins.is_running() {
            return None;
        }
        if let Some(rest) = content.strip_prefix('/') {
            let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if let Some(m) = self.plugins.command(name).cloned() {
                self.composer.clear();
                let guild = self.chat.channels.get(&c).and_then(|ch| ch.guild_id);
                let grant = self.plugins.grant(c.0, guild.map(|g| g.0), None);
                self.plugins.send(Job::Command {
                    id: m.id.clone(),
                    input: abi::CommandInput {
                        name: name.to_owned(),
                        args: args.trim().to_owned(),
                        channel_id: c.0,
                        guild_id: guild.map(|g| g.0),
                    },
                    grant,
                });
                return Some(Task::none());
            }
        }
        if !self.plugins.has_transform() || self.plugins.pending_transform.is_some() {
            return None;
        }
        let ticket = self.plugins.next_ticket();
        self.plugins.pending_transform = Some(plugins::PendingTransform {
            ticket,
            channel: c.0,
            content: content.to_owned(),
        });
        self.plugins.send(Job::Transform {
            ticket,
            input: abi::TransformInput {
                channel_id: c.0,
                text: content.to_owned(),
            },
        });
        self.composer.clear();
        Some(Task::perform(
            async { tokio::time::sleep(TRANSFORM_TIMEOUT).await },
            move |()| msg(PluginMsg::TransformTimeout(ticket)),
        ))
    }

    /// Gateway events observers may see.
    pub(super) fn plugin_observe(&self, ev: &Event) {
        if !self.plugins.is_running() {
            return;
        }
        let e = match ev {
            Event::MessageCreate(m) if self.plugins.wants("message_create") => {
                abi::Event::MessageCreate {
                    message: message_info(m, self.chat.me_id()),
                }
            }
            Event::MessageUpdate(p) => abi::Event::MessageUpdate {
                id: p.id.0,
                channel_id: p.channel_id.0,
                content: p.content.clone(),
            },
            Event::MessageDelete { channel_id, id } => abi::Event::MessageDelete {
                id: id.0,
                channel_id: channel_id.0,
            },
            Event::PresenceUpdate {
                user_id, status, ..
            } => abi::Event::Presence {
                user_id: user_id.0,
                status: plugins::presence_name(*status).into(),
            },
            _ => return,
        };
        self.plugins.observe(e);
    }

    /// The open channel changed (or a plugin just started): tell observers
    /// and hand them the messages already on screen.
    pub(super) fn plugin_channel_switch(&self) {
        if !self.plugins.is_running() {
            return;
        }
        let c = self.open_channel();
        let guild = match self.view {
            View::Guild { guild, .. } => Some(guild.0),
            View::Home(_) => None,
        };
        self.plugins.observe(abi::Event::ChannelSwitch {
            channel_id: c.map(|c| c.0),
            guild_id: guild,
        });
        if let Some(c) = c {
            self.plugin_history(c);
        }
    }

    /// The newest loaded messages of `c` (after a history page arrived).
    pub(super) fn plugin_history(&self, c: ChannelId) {
        if !self.plugins.is_running() || !self.plugins.wants("history") {
            return;
        }
        let Some(w) = self.chat.messages.get(&c) else {
            return;
        };
        let me = self.chat.me_id();
        let skip = w.messages.len().saturating_sub(HISTORY_EVENT_MAX);
        let messages: Vec<_> = w
            .messages
            .iter()
            .skip(skip)
            .filter(|m| !m.pending)
            .map(|m| message_info(m, me))
            .collect();
        if !messages.is_empty() {
            self.plugins.observe(abi::Event::History {
                channel_id: c.0,
                messages,
            });
        }
    }
}

/// Performs a plugin's HTTP request (already allowlist-checked), reading
/// at most [`MAX_HTTP_RESPONSE`] bytes.
async fn fetch(client: reqwest::Client, req: abi::HttpRequest) -> abi::HttpResponse {
    let fail = |e: &str| abi::HttpResponse {
        error: Some(e.to_owned()),
        ..Default::default()
    };
    let method = if req.method == "POST" {
        reqwest::Method::POST
    } else {
        reqwest::Method::GET
    };
    let mut b = client.request(method, &req.url);
    for (k, v) in &req.headers {
        b = b.header(k, v);
    }
    if !req.body.is_empty() {
        b = b.body(req.body);
    }
    let resp = match b.send().await {
        Ok(r) => r,
        Err(e) if e.is_timeout() => return fail("timed out"),
        Err(_) => return fail("request failed"),
    };
    let status = resp.status().as_u16();
    if resp
        .content_length()
        .is_some_and(|l| l > MAX_HTTP_RESPONSE as u64)
    {
        return fail("response too large");
    }
    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return fail("connection lost");
        };
        if body.len() + chunk.len() > MAX_HTTP_RESPONSE {
            return fail("response too large");
        }
        body.extend_from_slice(&chunk);
    }
    abi::HttpResponse {
        request_id: 0,
        status,
        body,
        error: None,
    }
}
