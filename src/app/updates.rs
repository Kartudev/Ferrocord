//! Glue between `App` and the updater (`crate::update`): timers, checks,
//! the install task and restart. The logic lives in `crate::update`.

use std::time::Duration;

use iced::Task;

use super::{App, Message, Ui};
use crate::config::{Config, ScreenshotScreen};
use crate::store::Store;
use crate::update::{
    self, CheckOutcome, CheckStatus, Endpoint, InstallEvent, InstallMethod, Phase, UpdateMsg,
    schedule,
};

fn msg(m: UpdateMsg) -> Message {
    Message::Update(m)
}

/// Builds the updater state at boot (reads a few `meta` rows).
pub(super) fn new_updater(cfg: &Config, store: &Store) -> update::Updater {
    let allowed = schedule::auto_allowed(
        cfg.mock,
        cfg.screenshot.is_some(),
        cfg!(debug_assertions),
        schedule::force_from_env(),
    );
    let persisted = if cfg.mock {
        update::Persisted {
            auto: true,
            ..Default::default()
        }
    } else {
        update::Persisted::load(store)
    };
    update::Updater::new(persisted, allowed)
}

impl App {
    /// Start-up: the first timed check 10–20 s from now and, off the UI
    /// thread, install-method detection plus cleanup of a previous
    /// update's leftovers. Nothing at all in mock / screenshot / debug.
    pub(super) fn boot_updater(&mut self) -> Task<Message> {
        if !self.updater.allowed {
            return Task::none();
        }
        let cleanup = Task::perform(
            async {
                let (method, exe) = update::detect_async(None).await;
                if let Some(dir) = update::install_dir(&method).map(std::path::Path::to_path_buf) {
                    let _ =
                        tokio::task::spawn_blocking(move || update::install::cleanup(&dir)).await;
                }
                (method, exe)
            },
            |(method, exe)| msg(UpdateMsg::Detected { method, exe }),
        );
        let first = if self.updater.auto {
            self.schedule_check(schedule::first_delay(fastrand::f64()))
        } else {
            Task::none()
        };
        Task::batch([cleanup, first])
    }

    /// Arms the timed check (replacing any pending one).
    fn schedule_check(&mut self, delay: Duration) -> Task<Message> {
        self.updater.timer += 1;
        let generation = self.updater.timer;
        tracing::debug!(secs = delay.as_secs(), "update: next check scheduled");
        Task::perform(async move { tokio::time::sleep(delay).await }, move |()| {
            msg(UpdateMsg::Tick(generation))
        })
    }

    fn start_check(&mut self, manual: bool) -> Task<Message> {
        self.updater.phase = Phase::Checking { manual };
        let etag = self.updater.etag.clone();
        let exe = self.updater.exe.clone();
        Task::perform(
            update::run_check(Endpoint::from_env(), etag, exe),
            move |(method, exe, result)| {
                msg(UpdateMsg::Checked {
                    manual,
                    method,
                    exe,
                    result,
                })
            },
        )
    }

    /// Settings → Updates opened: show the install method.
    pub(super) fn updates_page_opened(&mut self) -> Task<Message> {
        if !self.updater.allowed || self.updater.method.is_some() {
            return Task::none();
        }
        let exe = self.updater.exe.clone();
        Task::perform(update::detect_async(exe), |(method, exe)| {
            msg(UpdateMsg::Detected { method, exe })
        })
    }

    fn persist_update(&self, pairs: Vec<(&'static str, String)>) -> Task<Message> {
        if self.cfg.mock {
            return Task::none();
        }
        let store = self.store.clone();
        Task::perform(
            async move {
                store
                    .blocking(move |s| {
                        for (k, v) in &pairs {
                            update::save_meta(s, k, v);
                        }
                        Ok(())
                    })
                    .await
            },
            |_| Message::Done(super::Done::Ignored),
        )
    }

    pub(super) fn on_update(&mut self, m: UpdateMsg) -> Task<Message> {
        match m {
            UpdateMsg::Tick(generation) => {
                let u = &self.updater;
                if generation != u.timer || !u.auto || !u.allowed {
                    return Task::none();
                }
                if u.phase.busy() || matches!(u.phase, Phase::Ready { .. }) {
                    return self.schedule_check(schedule::next_delay(fastrand::f64()));
                }
                self.start_check(false)
            }
            UpdateMsg::CheckNow => {
                if !self.updater.allowed {
                    self.updater.status = CheckStatus::Off;
                    return Task::none();
                }
                if self.updater.phase.busy() {
                    return Task::none();
                }
                self.start_check(true)
            }
            UpdateMsg::Checked {
                manual,
                method,
                exe,
                result,
            } => {
                let u = &mut self.updater;
                u.method = Some(method);
                if u.exe.is_none() {
                    u.exe = exe;
                }
                let changed = u.apply_check(&result);
                match &result {
                    Ok(CheckOutcome::RateLimited(_)) => {
                        tracing::info!(manual, "update check rate-limited; backing off");
                    }
                    Err(e) => tracing::warn!(manual, error = %e, "update check failed"),
                    Ok(_) => match u.available() {
                        Some(r) => tracing::info!(manual, version = %r.version, "update available"),
                        None => tracing::info!(manual, "no update available"),
                    },
                }
                let save = if changed {
                    let release = u
                        .latest
                        .as_ref()
                        .and_then(|r| serde_json::to_string(r).ok())
                        .unwrap_or_default();
                    let etag = u.etag.clone().unwrap_or_default();
                    self.persist_update(vec![
                        (update::KEY_RELEASE, release),
                        (update::KEY_ETAG, etag),
                    ])
                } else {
                    Task::none()
                };
                let next = if self.updater.auto {
                    let delay = match &result {
                        Ok(CheckOutcome::RateLimited(d)) => schedule::backoff(*d),
                        Err(_) => schedule::MIN_BACKOFF,
                        Ok(_) => schedule::next_delay(fastrand::f64()),
                    };
                    self.schedule_check(delay)
                } else {
                    Task::none()
                };
                Task::batch([save, next])
            }
            UpdateMsg::Detected { method, exe } => {
                if self.updater.method.is_none() {
                    self.updater.method = Some(method);
                }
                if self.updater.exe.is_none() {
                    self.updater.exe = exe;
                }
                Task::none()
            }
            UpdateMsg::Start => {
                let u = &self.updater;
                if !u.allowed || u.phase.busy() || !u.can_self_update() {
                    return Task::none();
                }
                let (Some(release), Some(method)) = (u.available().cloned(), u.method.clone())
                else {
                    return Task::none();
                };
                tracing::info!(version = %release.version, "update: installing");
                self.updater.phase = Phase::Downloading {
                    received: 0,
                    total: None,
                };
                self.updater.restart_later = false;
                let (task, handle) = Task::run(
                    update::install_stream(Endpoint::from_env(), release, method),
                    |ev| msg(UpdateMsg::Install(ev)),
                )
                .abortable();
                self.updater.cancel = Some(handle);
                task
            }
            UpdateMsg::Install(ev) => {
                let u = &mut self.updater;
                if !u.phase.installing() {
                    // A late event after Cancel.
                    return Task::none();
                }
                u.phase = match ev {
                    InstallEvent::Progress(received, total) => {
                        Phase::Downloading { received, total }
                    }
                    InstallEvent::Verifying => Phase::Verifying,
                    InstallEvent::Installing => Phase::Installing,
                    InstallEvent::Done(Ok(version)) => {
                        u.cancel = None;
                        Phase::Ready { version }
                    }
                    InstallEvent::Done(Err(e)) => {
                        u.cancel = None;
                        Phase::Failed(e.to_string())
                    }
                };
                Task::none()
            }
            UpdateMsg::Cancel => {
                if matches!(self.updater.phase, Phase::Downloading { .. }) {
                    if let Some(h) = self.updater.cancel.take() {
                        h.abort();
                    }
                    tracing::info!("update: download cancelled");
                    self.updater.phase = Phase::Idle;
                }
                Task::none()
            }
            UpdateMsg::Dismiss => match self.updater.dismiss() {
                Some(v) => self.persist_update(vec![(update::KEY_DISMISSED, v.to_string())]),
                None => Task::none(),
            },
            UpdateMsg::Later => {
                self.updater.restart_later = true;
                Task::none()
            }
            UpdateMsg::Restart => self.restart_after_update(),
            UpdateMsg::ToggleAuto => {
                self.updater.auto = !self.updater.auto;
                let on = self.updater.auto;
                let save = self.persist_update(vec![(
                    update::KEY_AUTO,
                    if on { "1" } else { "0" }.to_owned(),
                )]);
                let timer = if on && self.updater.allowed {
                    self.schedule_check(schedule::first_delay(fastrand::f64()))
                } else {
                    // Invalidate the pending timer.
                    self.updater.timer += 1;
                    Task::none()
                };
                Task::batch([save, timer])
            }
            UpdateMsg::OpenReleasePage => {
                Task::done(Message::Ui(Ui::OpenUrl(self.updater.release_page())))
            }
            UpdateMsg::CopyCommand => iced::clipboard::write(update::method::cargo_command()),
            UpdateMsg::CleanedUp => Task::none(),
        }
    }

    /// [Restart now]: starts the installed binary with the same arguments
    /// (plus `--minimized` when hidden) after giving up the single-instance
    /// lock, then quits through the normal quit path.
    fn restart_after_update(&mut self) -> Task<Message> {
        if !matches!(self.updater.phase, Phase::Ready { .. }) || self.cfg.mock {
            return Task::none();
        }
        let Some(target) =
            update::restart_target(self.updater.method.as_ref(), self.updater.exe.as_deref())
        else {
            return Task::none();
        };
        let args: Vec<String> = std::env::args().skip(1).collect();
        let args = update::restart_args(&args, !self.win.visible);
        crate::tray::instance::release();
        match update::spawn_restart(&target, &args) {
            Ok(()) => {
                // `spawn` has returned, so the new process exists (on Unix it
                // has exec'd): only now may Quit arm the exit watchdog.
                tracing::info!("update: restarting into the new version");
                self.quit()
            }
            Err(e) => {
                tracing::warn!(error = %e, "update: could not start the new version");
                self.updater.phase = Phase::Failed(format!(
                    "could not start the new version ({e}); it will run on the next start"
                ));
                Task::none()
            }
        }
    }

    /// Mock screenshots: a newer release found for an `install.sh` copy.
    pub(super) fn mock_update_screen(&mut self, screen: Option<ScreenshotScreen>) {
        let wanted = matches!(
            screen,
            Some(ScreenshotScreen::UpdateBanner)
                | Some(ScreenshotScreen::SettingsPage(
                    super::settings::Category::Updates
                ))
        );
        if !wanted {
            return;
        }
        let next = update::Version {
            minor: self.updater.current.minor + 1,
            patch: 0,
            ..self.updater.current
        };
        self.updater.latest = Some(update::Release {
            tag: format!("v{next}"),
            version: next,
            html_url: format!("{}/tag/v{next}", update::releases_page()),
            assets: Vec::new(),
        });
        self.updater.method = Some(InstallMethod::Script(update::ScriptLayout {
            bin_dir: "~/.local/bin".into(),
            data_dir: "~/.local/share".into(),
        }));
        self.updater.status = CheckStatus::Available(next);
    }
}
