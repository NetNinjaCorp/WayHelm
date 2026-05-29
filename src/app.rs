use adw::prelude::*;
use gtk::glib;

use crate::ui::{dashboard, wizard};
use crate::{config, service, settings, tray};

pub fn on_activate(app: &adw::Application) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Wayhelm")
        .default_width(820)
        .default_height(720)
        .build();

    // SNI tray icon: lives for the whole process. Menu actions arrive on the
    // GTK main loop via async_channel and dispatch back into this module.
    let tray_rx = tray::spawn();
    {
        let window = window.clone();
        let app = app.clone();
        glib::spawn_future_local(async move {
            while let Ok(cmd) = tray_rx.recv().await {
                match cmd {
                    tray::TrayCmd::Show => {
                        window.set_visible(true);
                        window.present();
                    }
                    tray::TrayCmd::Start => {
                        let _ = service::start();
                    }
                    tray::TrayCmd::Stop => {
                        let _ = service::stop();
                    }
                    tray::TrayCmd::Quit => {
                        app.quit();
                    }
                }
            }
        });
    }

    // Close-to-tray. First press asks; subsequent presses respect the saved
    // choice. Hidden windows keep the GTK application alive on their own --
    // no app.hold() needed.
    {
        let app = app.clone();
        window.connect_close_request(move |w| {
            let s = settings::Settings::load();
            match s.close_action {
                Some(settings::CloseAction::Quit) => glib::Propagation::Proceed,
                Some(settings::CloseAction::HideToTray) => {
                    w.set_visible(false);
                    glib::Propagation::Stop
                }
                None => {
                    show_close_dialog(w, &app);
                    glib::Propagation::Stop
                }
            }
        });
    }

    let cfg = config::Config::load_or_default().unwrap_or_default();
    if cfg.is_configured() {
        show_dashboard(&window);
    } else {
        show_wizard(&window);
    }
    // `--hidden` (used by the autostart .desktop) keeps the window invisible
    // until the user clicks the tray icon. The window still exists, so the
    // GTK application stays alive and the periodic refresh closures continue
    // updating tray state.
    if !crate::START_HIDDEN.load(std::sync::atomic::Ordering::Relaxed) {
        window.present();
    }
}

pub fn show_wizard(window: &adw::ApplicationWindow) {
    window.set_content(Some(&wizard::build(window)));
}

pub fn show_dashboard(window: &adw::ApplicationWindow) {
    window.set_content(Some(&dashboard::build(window)));
}

fn show_close_dialog(window: &adw::ApplicationWindow, app: &adw::Application) {
    let dialog = adw::AlertDialog::builder()
        .heading("Close Wayhelm")
        .body(
            "Wayhelm can keep running in the background and show a tray icon — \
             handy for monitoring connections and starting or stopping wayvnc \
             without reopening the app.\n\n\
             What should the close button do from now on?",
        )
        .build();

    dialog.add_response("hide", "Hide to tray");
    dialog.set_response_appearance("hide", adw::ResponseAppearance::Suggested);
    dialog.add_response("quit", "Quit");
    dialog.add_response("cancel", "Cancel");
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("hide"));

    {
        let window = window.clone();
        let app = app.clone();
        dialog.connect_response(None, move |_, response| {
            let mut s = settings::Settings::load();
            match response {
                "hide" => {
                    s.close_action = Some(settings::CloseAction::HideToTray);
                    let _ = s.save();
                    window.set_visible(false);
                }
                "quit" => {
                    s.close_action = Some(settings::CloseAction::Quit);
                    let _ = s.save();
                    app.quit();
                }
                _ => { /* cancel: do nothing */ }
            }
        });
    }

    dialog.present(Some(window));
}
