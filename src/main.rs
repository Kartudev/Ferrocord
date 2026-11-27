//! Ferrocord entry point: logging, config, `iced::application(..).run()`.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use ferrocord::app::App;
use ferrocord::config::{APP_ID, Config};
use ferrocord::error::log_sink::LogSink;
use ferrocord::tray::instance::{self, Role};
use tracing_subscriber::EnvFilter;

/// Usage line shown by `ferrocord --help`.
const USAGE: &str =
    "ferrocord [--mock] [--minimized] [--no-tray] [--screenshot <screen> <out.png>]";

fn main() -> iced::Result {
    ferrocord::cli::handle_info_flags("ferrocord", USAGE);
    let _ = ferrocord::config::PROCESS_START.set(std::time::Instant::now());
    // Rendering is tiny-skia (CPU) only: ferroiced builds without wgpu,
    // which cost 100-200 MB RSS (driver dependent) for no visible gain in a
    // chat UI. To try wgpu again, enable iced's `wgpu` feature in
    // `Cargo.toml` and run with `ICED_BACKEND=wgpu`.
    let cfg = match Config::from_args(std::env::args()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,cosmic_text=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(LogSink::new(cfg.log_path()))
        .with_ansi(false)
        .init();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        mock = cfg.mock,
        "starting Ferrocord"
    );

    // One instance (and one tray icon): a second launch asks the running
    // one to show its window and exits. Screenshots never take the lock.
    if cfg.tray {
        let name = if cfg.mock {
            format!("{APP_ID}.mock")
        } else {
            APP_ID.to_owned()
        };
        match instance::acquire(&name, !cfg.minimized) {
            Role::Primary(p) => p.serve(instance::request_show_locally),
            Role::Secondary => {
                tracing::info!("already running; asked it to show its window");
                return Ok(());
            }
        }
    }

    // Toasts carry the app's own name instead of "Windows PowerShell".
    #[cfg(windows)]
    if !cfg.mock {
        ferrocord::win_app_id::register();
    }

    // A daemon, not an application: hiding to the tray closes the window
    // (freeing the renderer) while the app keeps running.
    let app = iced::daemon(
        move || App::boot(cfg.clone()),
        App::update,
        ferrocord::ui::view_window,
    )
    .title(|app: &App, _window: iced::window::Id| app.title())
    .subscription(App::subscription)
    // Appearance theme and zoom level change at run time.
    .theme(|app: &App, _window: iced::window::Id| app.theme.iced_theme())
    .scale_factor(|app: &App, _window: iced::window::Id| app.zoom())
    .default_font(ferrocord::ui::theme::INTER)
    .antialiasing(true);
    // UI font (Inter) and colour emoji (Twemoji), before any window exists
    // (see `ui::emoji_font`).
    ferrocord::ui::emoji_font::install();
    app.run()
}
