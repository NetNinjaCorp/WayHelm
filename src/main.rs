mod app;
mod autostart;
mod certs;
mod config;
mod connlog;
mod ctl;
mod diagnostics;
mod firewall;
mod netinfo;
mod service;
mod settings;
mod tray;
mod ui;
mod util;

use std::sync::atomic::{AtomicBool, Ordering};

use adw::prelude::*;
use gtk::glib;

const APP_ID: &str = "io.github.wayhelm.Wayhelm";

/// Set by `--hidden` on the command line. Read in app::on_activate to decide
/// whether to present the window or stay tray-only on launch.
pub static START_HIDDEN: AtomicBool = AtomicBool::new(false);

fn main() -> glib::ExitCode {
    // GTK Application parses argv itself and errors on unknown flags, so we
    // strip `--hidden` before handing argv to it.
    let raw: Vec<String> = std::env::args().collect();
    let hidden = raw.iter().any(|a| a == "--hidden");
    START_HIDDEN.store(hidden, Ordering::Relaxed);
    let filtered: Vec<String> = raw.into_iter().filter(|a| a != "--hidden").collect();

    let application = adw::Application::builder().application_id(APP_ID).build();
    application.connect_activate(app::on_activate);
    application.run_with_args(&filtered)
}
