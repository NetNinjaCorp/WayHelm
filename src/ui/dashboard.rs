use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::{app as appmod, autostart, certs, config, connlog, ctl, diagnostics, firewall, netinfo, service, settings, tray, util};

pub fn build(window: &adw::ApplicationWindow) -> gtk::Widget {
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();

    let reconfig = gtk::Button::from_icon_name("view-refresh-symbolic");
    reconfig.set_tooltip_text(Some("Re-run setup wizard"));
    {
        let window = window.clone();
        reconfig.connect_clicked(move |_| {
            appmod::show_wizard(&window);
        });
    }
    header.pack_end(&reconfig);
    toolbar.add_top_bar(&header);

    let toast = adw::ToastOverlay::new();
    let cfg = config::Config::load_or_default().unwrap_or_default();

    maybe_add_firewall_banner(&toolbar, &toast, &cfg, window);

    // Crash banner — pre-built, hidden by default. The refresh tick toggles
    // its visibility based on a journal-derived crash count.
    let crash_banner = adw::Banner::new("");
    crash_banner.set_revealed(false);
    crash_banner.set_button_label(Some("Diagnose…"));
    {
        let window = window.clone();
        let toast = toast.clone();
        crash_banner.connect_button_clicked(move |_| {
            show_crash_dialog(&window, &toast);
        });
    }
    toolbar.add_top_bar(&crash_banner);

    // Three pages, switched via a ViewSwitcher in the header. Splits the old
    // single-scrolling dashboard into a glance/settings/troubleshooting flow.
    let stack = adw::ViewStack::new();

    let overview_prefs = adw::PreferencesPage::new();
    let settings_prefs = adw::PreferencesPage::new();
    let advanced_prefs = adw::PreferencesPage::new();

    // Overview — what you check at a glance: status, who's on, where they
    // connect to, which screen they see.
    let (status_lbl, start_btn, stop_btn, restart_btn, enable_row) =
        build_service_group(&overview_prefs, &toast);
    build_connection_group(&overview_prefs, &cfg, &toast);
    let clients_group = build_clients_group(&overview_prefs);
    let outputs_group = build_outputs_group(&overview_prefs);

    // Settings — credentials, certs, compatibility knobs. Things you change
    // occasionally and want to deliberate over.
    build_auth_mode_group(&settings_prefs, &toast);
    build_auth_group(&settings_prefs, &cfg, &toast);
    build_keyboard_group(&settings_prefs, &toast);
    build_tls_group(&settings_prefs, &cfg, &toast);
    build_wayvnc_options_group(&settings_prefs, &toast);
    build_app_prefs_group(&settings_prefs, &toast);

    // Advanced — troubleshooting and the underlying files.
    build_diagnostics_group(&advanced_prefs);
    build_connlog_group(&advanced_prefs);
    build_raw_config_group(&advanced_prefs, &cfg);
    build_logs_group(&advanced_prefs);

    let overview_scroll = scrolled_page(&overview_prefs);
    let settings_scroll = scrolled_page(&settings_prefs);
    let advanced_scroll = scrolled_page(&advanced_prefs);

    stack
        .add_titled_with_icon(&overview_scroll, Some("overview"), "Overview", "view-grid-symbolic");
    stack
        .add_titled_with_icon(&settings_scroll, Some("settings"), "Settings", "emblem-system-symbolic");
    stack
        .add_titled_with_icon(&advanced_scroll, Some("advanced"), "Advanced", "applications-utilities-symbolic");

    let switcher = adw::ViewSwitcher::builder()
        .stack(&stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    header.set_title_widget(Some(&switcher));

    toast.set_child(Some(&stack));
    toolbar.set_content(Some(&toast));

    let refresh_status = {
        let status_lbl = status_lbl.clone();
        let start_btn = start_btn.clone();
        let stop_btn = stop_btn.clone();
        let restart_btn = restart_btn.clone();
        let enable_row = enable_row.clone();
        let enable_row_guard = Rc::new(RefCell::new(true));
        move || {
            let s = service::status();
            let text = if !s.installed {
                "Not installed".to_string()
            } else if s.active {
                if s.sub_state.is_empty() {
                    "Running".into()
                } else {
                    format!("Running ({})", s.sub_state)
                }
            } else if s.sub_state.is_empty() {
                "Stopped".into()
            } else {
                format!("Stopped ({})", s.sub_state)
            };
            status_lbl.set_label(&text);
            start_btn.set_sensitive(s.installed && !s.active);
            stop_btn.set_sensitive(s.installed && s.active);
            restart_btn.set_sensitive(s.installed && s.active);

            // Update the enable switch without re-firing its toggle handler.
            *enable_row_guard.borrow_mut() = false;
            if enable_row.is_active() != s.enabled {
                enable_row.set_active(s.enabled);
            }
            *enable_row_guard.borrow_mut() = true;
        }
    };
    refresh_status();

    let outputs_holder: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::new(RefCell::new(Vec::new()));
    let refresh_outputs = {
        let group = outputs_group.clone();
        let holder = outputs_holder.clone();
        let toast = toast.clone();
        move || {
            let mut h = holder.borrow_mut();
            for r in h.drain(..) {
                group.remove(&r);
            }
            if !ctl::is_running() {
                let r = adw::ActionRow::builder()
                    .title("(wayvnc not running)")
                    .build();
                r.add_css_class("dim-label");
                group.add(&r);
                h.push(r);
                return;
            }
            match ctl::output_list() {
                Err(e) => {
                    let r = adw::ActionRow::builder()
                        .title(format!("Couldn't list outputs: {e}"))
                        .build();
                    group.add(&r);
                    h.push(r);
                }
                Ok(outputs) if outputs.is_empty() => {
                    let r = adw::ActionRow::builder()
                        .title("No outputs reported")
                        .build();
                    group.add(&r);
                    h.push(r);
                }
                Ok(outputs) => {
                    for o in outputs {
                        let title = match &o.description {
                            Some(d) if !d.is_empty() => format!("{} — {}", o.name, d),
                            _ => o.name.clone(),
                        };
                        let r = adw::ActionRow::builder()
                            .title(&title)
                            .subtitle(if o.captured { "Currently captured" } else { "Available" })
                            .build();
                        if o.captured {
                            let icon = gtk::Image::from_icon_name("emblem-ok-symbolic");
                            icon.set_valign(gtk::Align::Center);
                            r.add_suffix(&icon);
                        } else {
                            let btn = gtk::Button::with_label("Capture");
                            btn.set_valign(gtk::Align::Center);
                            let name = o.name.clone();
                            let toast = toast.clone();
                            btn.connect_clicked(move |_| {
                                if let Err(e) = ctl::output_set(&name) {
                                    toast.add_toast(adw::Toast::new(&format!(
                                        "output-set failed: {e}"
                                    )));
                                }
                            });
                            r.add_suffix(&btn);
                        }
                        group.add(&r);
                        h.push(r);
                    }
                }
            }
        }
    };
    refresh_outputs();

    let clients_holder: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::new(RefCell::new(Vec::new()));
    let refresh_clients = {
        let group = clients_group.clone();
        let holder = clients_holder.clone();
        move || {
            let mut h = holder.borrow_mut();
            for r in h.drain(..) {
                group.remove(&r);
            }
            if !ctl::is_running() {
                let r = adw::ActionRow::builder()
                    .title("(wayvnc not running)")
                    .build();
                r.add_css_class("dim-label");
                group.add(&r);
                h.push(r);
                return;
            }
            let clients = ctl::client_list().unwrap_or_default();
            if clients.is_empty() {
                let r = adw::ActionRow::builder().title("No clients connected").build();
                r.add_css_class("dim-label");
                group.add(&r);
                h.push(r);
                return;
            }
            for c in clients {
                let who = c.username.as_deref().unwrap_or("(anon)");
                let where_ = c.address.as_deref().unwrap_or("?");
                let seat = c
                    .seat
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|s| format!(" · seat {s}"))
                    .unwrap_or_default();
                let r = adw::ActionRow::builder()
                    .title(format!("Client {}", c.id))
                    .subtitle(format!("{who} @ {where_}{seat}"))
                    .build();
                let dis = gtk::Button::with_label("Disconnect");
                dis.add_css_class("destructive-action");
                dis.set_valign(gtk::Align::Center);
                let id = c.id.clone();
                dis.connect_clicked(move |_| {
                    let _ = ctl::client_disconnect(&id);
                });
                r.add_suffix(&dis);
                group.add(&r);
                h.push(r);
            }
        }
    };
    refresh_clients();

    let refresh_crash_banner = {
        let banner = crash_banner.clone();
        move || {
            let crashes = service::recent_crash_count(5);
            if crashes >= 3 {
                banner.set_title(&format!(
                    "wayvnc has crashed {crashes} times in the last 5 minutes — likely a client-triggered bug."
                ));
                banner.set_revealed(true);
            } else {
                banner.set_revealed(false);
            }
        }
    };
    refresh_crash_banner();

    // Tracks clients we've already logged a CONNECT for. Initialized from the
    // current state so we don't write spurious connect events for sessions
    // that were already in progress when Wayhelm launched.
    let known_clients: Rc<RefCell<HashMap<String, connlog::ConnInfo>>> =
        Rc::new(RefCell::new(HashMap::new()));
    if ctl::is_running() {
        let mut k = known_clients.borrow_mut();
        let now = std::time::SystemTime::now();
        for c in ctl::client_list().unwrap_or_default() {
            k.insert(
                c.id.clone(),
                connlog::ConnInfo {
                    id: c.id,
                    address: c.address,
                    username: c.username,
                    started_at: now,
                },
            );
        }
    }

    glib::timeout_add_seconds_local(2, move || {
        refresh_status();
        refresh_clients();
        refresh_outputs();
        refresh_crash_banner();

        let active = service::status().active;
        let current_clients: Vec<ctl::Client> = if ctl::is_running() {
            ctl::client_list().unwrap_or_default()
        } else {
            Vec::new()
        };

        // Diff against the previous tick's clients to record connect /
        // disconnect events in the connection log.
        {
            let mut k = known_clients.borrow_mut();
            if !active {
                for (_, info) in k.drain() {
                    let _ = connlog::append_disconnect(&info);
                }
            } else {
                let now = std::time::SystemTime::now();
                let mut current_ids =
                    std::collections::HashSet::with_capacity(current_clients.len());
                for c in &current_clients {
                    current_ids.insert(c.id.clone());
                    if !k.contains_key(&c.id) {
                        let info = connlog::ConnInfo {
                            id: c.id.clone(),
                            address: c.address.clone(),
                            username: c.username.clone(),
                            started_at: now,
                        };
                        let _ = connlog::append_connect(&info);
                        k.insert(c.id.clone(), info);
                    }
                }
                let gone: Vec<String> = k
                    .keys()
                    .filter(|id| !current_ids.contains(*id))
                    .cloned()
                    .collect();
                for id in gone {
                    if let Some(info) = k.remove(&id) {
                        let _ = connlog::append_disconnect(&info);
                    }
                }
            }
        }

        let labels: Vec<String> = current_clients
            .iter()
            .map(|c| {
                let who = c.username.as_deref().unwrap_or("(anon)");
                let where_ = c.address.as_deref().unwrap_or("?");
                format!("{who} @ {where_}")
            })
            .collect();
        tray::update_status(active, &labels);

        glib::ControlFlow::Continue
    });

    toolbar.upcast()
}

// ---------------------------------------------------------------------------
// Groups
// ---------------------------------------------------------------------------

fn build_service_group(
    prefs: &adw::PreferencesPage,
    toast: &adw::ToastOverlay,
) -> (
    gtk::Label,
    gtk::Button,
    gtk::Button,
    gtk::Button,
    adw::SwitchRow,
) {
    let group = adw::PreferencesGroup::builder().title("Service").build();

    let status_row = adw::ActionRow::builder().title("Status").build();
    let status_lbl = gtk::Label::new(Some("…"));
    status_lbl.add_css_class("dim-label");
    status_row.add_suffix(&status_lbl);
    group.add(&status_row);

    let action_row = adw::ActionRow::builder().title("Controls").build();
    let start_btn = gtk::Button::with_label("Start");
    start_btn.add_css_class("suggested-action");
    let stop_btn = gtk::Button::with_label("Stop");
    let restart_btn = gtk::Button::with_label("Restart");
    for b in [&start_btn, &stop_btn, &restart_btn] {
        b.set_valign(gtk::Align::Center);
    }
    let btns = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    btns.append(&start_btn);
    btns.append(&stop_btn);
    btns.append(&restart_btn);
    action_row.add_suffix(&btns);
    group.add(&action_row);

    let enable_row = adw::SwitchRow::builder()
        .title("Start automatically on login")
        .build();
    group.add(&enable_row);

    {
        let toast = toast.clone();
        start_btn.connect_clicked(move |_| {
            if let Err(e) = service::start() {
                toast.add_toast(adw::Toast::new(&format!("Start failed: {e}")));
            }
        });
    }
    {
        let toast = toast.clone();
        stop_btn.connect_clicked(move |_| {
            if let Err(e) = service::stop() {
                toast.add_toast(adw::Toast::new(&format!("Stop failed: {e}")));
            }
        });
    }
    {
        let toast = toast.clone();
        restart_btn.connect_clicked(move |_| {
            if let Err(e) = service::restart() {
                toast.add_toast(adw::Toast::new(&format!("Restart failed: {e}")));
            }
        });
    }
    {
        let toast = toast.clone();
        enable_row.connect_active_notify(move |row| {
            let res = if row.is_active() {
                service::enable()
            } else {
                service::disable()
            };
            if let Err(e) = res {
                toast.add_toast(adw::Toast::new(&format!("systemctl: {e}")));
            }
        });
    }

    prefs.add(&group);
    (status_lbl, start_btn, stop_btn, restart_btn, enable_row)
}

fn build_connection_group(
    prefs: &adw::PreferencesPage,
    cfg: &config::Config,
    toast: &adw::ToastOverlay,
) {
    let group = adw::PreferencesGroup::builder()
        .title("Connecting")
        .description(
            "Use any VNC client (Remmina, TigerVNC, RealVNC, KRDC). \
             On the first connection you'll see a TLS warning — match the fingerprint below, trust it, and you won't be asked again.",
        )
        .build();

    let port = cfg.port.unwrap_or(5900);
    let bound = cfg
        .address
        .clone()
        .unwrap_or_else(|| "127.0.0.1".into());

    let bound_row = adw::ActionRow::builder()
        .title("Listening on")
        .subtitle(format!("{bound}:{port}"))
        .build();
    group.add(&bound_row);

    let lan = bound != "127.0.0.1" && bound.to_ascii_lowercase() != "localhost";
    if !lan {
        let r = adw::ActionRow::builder()
            .title("Loopback only")
            .subtitle(format!(
                "Reachable only from this machine. To connect remotely, tunnel through SSH:\nssh -L {port}:127.0.0.1:{port} you@this-host"
            ))
            .build();
        group.add(&r);
    } else {
        let addrs = netinfo::local_addresses();
        if addrs.is_empty() {
            let r = adw::ActionRow::builder()
                .title("No interfaces detected")
                .build();
            group.add(&r);
        } else {
            for iface in addrs {
                let url = format!("vnc://{}:{}", maybe_brackets(&iface.addr.to_string()), port);
                let r = adw::ActionRow::builder()
                    .title(format!("{} ({})", iface.addr, iface.name))
                    .subtitle(&url)
                    .build();
                let copy = gtk::Button::from_icon_name("edit-copy-symbolic");
                copy.add_css_class("flat");
                copy.set_valign(gtk::Align::Center);
                copy.set_tooltip_text(Some("Copy connection URL"));
                let url_clone = url.clone();
                let toast = toast.clone();
                copy.connect_clicked(move |w| {
                    w.clipboard().set_text(&url_clone);
                    toast.add_toast(adw::Toast::new("Copied"));
                });
                r.add_suffix(&copy);
                group.add(&r);
            }
        }
    }

    prefs.add(&group);
}

fn build_auth_group(
    prefs: &adw::PreferencesPage,
    cfg: &config::Config,
    toast: &adw::ToastOverlay,
) {
    let group = adw::PreferencesGroup::builder().title("Authentication").build();

    let user_row = adw::ActionRow::builder()
        .title("Username")
        .subtitle(cfg.username.clone().unwrap_or_else(|| "(not set)".into()))
        .build();
    group.add(&user_row);

    let pass_row = adw::ActionRow::builder()
        .title("Password")
        .subtitle("••••••••")
        .build();

    let reveal = gtk::Button::with_label("Reveal");
    reveal.add_css_class("flat");
    reveal.set_valign(gtk::Align::Center);
    {
        let original_pw = cfg.password.clone().unwrap_or_default();
        let pass_row_w = pass_row.clone();
        reveal.connect_clicked(move |b| {
            if b.label().as_deref() == Some("Reveal") {
                pass_row_w.set_subtitle(&original_pw);
                b.set_label("Hide");
            } else {
                pass_row_w.set_subtitle("••••••••");
                b.set_label("Reveal");
            }
        });
    }
    pass_row.add_suffix(&reveal);

    let rotate = gtk::Button::with_label("Rotate");
    rotate.add_css_class("flat");
    rotate.set_valign(gtk::Align::Center);
    rotate.set_tooltip_text(Some("Generate a new strong password and restart wayvnc"));
    {
        let pass_row_w = pass_row.clone();
        let reveal_w = reveal.clone();
        let toast = toast.clone();
        rotate.connect_clicked(move |_| {
            let new = util::random_password(16);
            let mut c = match config::Config::load_or_default() {
                Ok(c) => c,
                Err(e) => {
                    toast.add_toast(adw::Toast::new(&format!("Load failed: {e}")));
                    return;
                }
            };
            c.password = Some(new.clone());
            if let Err(e) = c.save() {
                toast.add_toast(adw::Toast::new(&format!("Save failed: {e}")));
                return;
            }
            pass_row_w.set_subtitle(&new);
            reveal_w.set_label("Hide");
            if let Err(e) = service::restart() {
                toast.add_toast(adw::Toast::new(&format!(
                    "Saved, but restart failed: {e}"
                )));
            } else {
                toast.add_toast(adw::Toast::new("Password rotated, service restarted"));
            }
        });
    }
    pass_row.add_suffix(&rotate);

    group.add(&pass_row);
    prefs.add(&group);
}

fn build_tls_group(prefs: &adw::PreferencesPage, cfg: &config::Config, toast: &adw::ToastOverlay) {
    let group = adw::PreferencesGroup::builder().title("TLS certificate").build();

    let fp = cfg
        .certificate_file
        .as_deref()
        .and_then(|p| certs::fingerprint(p).ok())
        .unwrap_or_else(|| "(unknown)".into());
    let exp = cfg
        .certificate_file
        .as_deref()
        .and_then(|p| certs::not_after(p).ok())
        .unwrap_or_else(|| "(unknown)".into());

    let fp_row = adw::ActionRow::builder().title("Fingerprint").subtitle(&fp).build();
    fp_row.set_subtitle_selectable(true);
    group.add(&fp_row);

    let exp_row = adw::ActionRow::builder().title("Expires").subtitle(&exp).build();
    group.add(&exp_row);

    let regen_row = adw::ActionRow::builder()
        .title("Regenerate certificate")
        .subtitle("Generates a new self-signed cert and RSA key, then restarts wayvnc")
        .build();
    let regen_btn = gtk::Button::with_label("Regenerate");
    regen_btn.add_css_class("destructive-action");
    regen_btn.set_valign(gtk::Align::Center);
    {
        let toast = toast.clone();
        let fp_row = fp_row.clone();
        let exp_row = exp_row.clone();
        regen_btn.connect_clicked(move |_| {
            match certs::generate(&netinfo::hostname(), 825) {
                Ok(p) => {
                    fp_row.set_subtitle(&certs::fingerprint(&p.cert).unwrap_or_default());
                    exp_row.set_subtitle(&certs::not_after(&p.cert).unwrap_or_default());
                    if let Err(e) = service::restart() {
                        toast.add_toast(adw::Toast::new(&format!(
                            "Regenerated, but restart failed: {e}"
                        )));
                    } else {
                        toast.add_toast(adw::Toast::new("Certificate regenerated, service restarted"));
                    }
                }
                Err(e) => {
                    toast.add_toast(adw::Toast::new(&format!("openssl failed: {e}")));
                }
            }
        });
    }
    regen_row.add_suffix(&regen_btn);
    group.add(&regen_row);

    prefs.add(&group);
}

fn build_keyboard_group(prefs: &adw::PreferencesPage, toast: &adw::ToastOverlay) {
    let cfg = config::Config::load_or_default().unwrap_or_default();
    let group = adw::PreferencesGroup::builder()
        .title("Keyboard layout")
        .description(
            "How wayvnc interprets keystrokes from connecting clients. Defaults to your \
             system XKB settings; override here if remote typing comes through wrong. \
             Writes xkb_* keys to ~/.config/wayvnc/config and restarts wayvnc.",
        )
        .build();

    let layout = adw::EntryRow::builder().title("Layout (e.g. us, de, fr, gb)").build();
    if let Some(v) = &cfg.xkb_layout {
        layout.set_text(v);
    }
    group.add(&layout);

    let variant = adw::EntryRow::builder()
        .title("Variant (optional, e.g. intl, dvorak, nodeadkeys)")
        .build();
    if let Some(v) = &cfg.xkb_variant {
        variant.set_text(v);
    }
    group.add(&variant);

    let model = adw::EntryRow::builder().title("Model (default pc105)").build();
    if let Some(v) = &cfg.xkb_model {
        model.set_text(v);
    }
    group.add(&model);

    let options = adw::EntryRow::builder()
        .title("Options (comma-separated, e.g. ctrl:swapcaps,compose:rwin)")
        .build();
    if let Some(v) = &cfg.xkb_options {
        options.set_text(v);
    }
    group.add(&options);

    let apply_row = adw::ActionRow::builder().title("Apply keyboard settings").build();
    let btn = gtk::Button::with_label("Apply");
    btn.add_css_class("suggested-action");
    btn.set_valign(gtk::Align::Center);
    apply_row.add_suffix(&btn);
    group.add(&apply_row);

    {
        let layout = layout.clone();
        let variant = variant.clone();
        let model = model.clone();
        let options = options.clone();
        let toast = toast.clone();
        btn.connect_clicked(move |_| {
            let mut cfg = match config::Config::load_or_default() {
                Ok(c) => c,
                Err(e) => {
                    toast.add_toast(adw::Toast::new(&format!("Config load failed: {e}")));
                    return;
                }
            };
            let to_opt = |s: String| -> Option<String> {
                let t = s.trim();
                if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                }
            };
            cfg.xkb_layout = to_opt(layout.text().to_string());
            cfg.xkb_variant = to_opt(variant.text().to_string());
            cfg.xkb_model = to_opt(model.text().to_string());
            cfg.xkb_options = to_opt(options.text().to_string());
            if let Err(e) = cfg.save() {
                toast.add_toast(adw::Toast::new(&format!("Config save failed: {e}")));
                return;
            }
            if let Err(e) = service::restart() {
                toast.add_toast(adw::Toast::new(&format!(
                    "Saved, but restart failed: {e}"
                )));
                return;
            }
            toast.add_toast(adw::Toast::new("Keyboard layout applied. wayvnc restarted."));
        });
    }

    prefs.add(&group);
}

/// 3-way auth mode. Maps to (enable_auth, relax_encryption) in wayvnc's config.
/// "Disabled" is the only way to make TightVNC/UltraVNC connect to wayvnc
/// today (see neatvnc init_security_types: VNC Auth type 2 needs both
/// !REQUIRE_USERNAME and ALLOW_BROKEN_CRYPTO, neither of which wayvnc
/// exposes -- so plaintext-only NONE is the only legacy-client path).
fn build_auth_mode_group(prefs: &adw::PreferencesPage, toast: &adw::ToastOverlay) {
    let cfg = config::Config::load_or_default().unwrap_or_default();
    let current_idx: u32 = if !cfg.enable_auth {
        2
    } else if cfg.relax_encryption {
        1
    } else {
        0
    };

    let group = adw::PreferencesGroup::builder()
        .title("Authentication mode")
        .description(
            "Controls which RFB security types wayvnc announces. \"Disabled\" is what \
             you need for TightVNC / UltraVNC — wayvnc doesn't expose the flags neatvnc \
             needs to offer legacy VNC Auth alongside a password.",
        )
        .build();

    let model = gtk::StringList::new(&[
        "Encrypted connection required (best, default)",
        "Allow encrypted fallbacks (Apple DH, etc.)",
        "DISABLED — no password, no encryption (any client)",
    ]);
    let combo = adw::ComboRow::builder()
        .title("Mode")
        .subtitle("Restart wayvnc after changing.")
        .model(&model)
        .build();
    combo.set_selected(current_idx);
    group.add(&combo);

    let apply_row = adw::ActionRow::builder()
        .title("Apply mode change")
        .subtitle("Rewrites enable_auth / relax_encryption in ~/.config/wayvnc/config and restarts wayvnc")
        .build();
    let apply_btn = gtk::Button::with_label("Apply");
    apply_btn.add_css_class("suggested-action");
    apply_btn.set_valign(gtk::Align::Center);
    apply_row.add_suffix(&apply_btn);
    group.add(&apply_row);

    {
        let combo = combo.clone();
        let toast = toast.clone();
        apply_btn.connect_clicked(move |_| {
            let mut cfg = match config::Config::load_or_default() {
                Ok(c) => c,
                Err(e) => {
                    toast.add_toast(adw::Toast::new(&format!("Config load failed: {e}")));
                    return;
                }
            };
            match combo.selected() {
                0 => {
                    cfg.enable_auth = true;
                    cfg.relax_encryption = false;
                }
                1 => {
                    cfg.enable_auth = true;
                    cfg.relax_encryption = true;
                }
                2 => {
                    cfg.enable_auth = false;
                    cfg.relax_encryption = false;
                }
                _ => return,
            }
            if let Err(e) = cfg.save() {
                toast.add_toast(adw::Toast::new(&format!("Config save failed: {e}")));
                return;
            }
            if let Err(e) = service::restart() {
                toast.add_toast(adw::Toast::new(&format!(
                    "Saved, but restart failed: {e}"
                )));
                return;
            }
            let msg = match combo.selected() {
                0 => "Auth mode: encryption required. wayvnc restarted.",
                1 => "Auth mode: legacy fallbacks allowed. wayvnc restarted.",
                2 => "Auth DISABLED — anyone on the network can connect. wayvnc restarted.",
                _ => "wayvnc restarted.",
            };
            toast.add_toast(adw::Toast::new(msg));
        });
    }

    prefs.add(&group);
}

fn build_wayvnc_options_group(prefs: &adw::PreferencesPage, toast: &adw::ToastOverlay) {
    let s = settings::Settings::load();
    let group = adw::PreferencesGroup::builder()
        .title("WayVNC server options")
        .description(
            "Behavior tunables for wayvnc itself. Click Apply to rewrite the systemd \
             unit's ExecStart line and restart the service.",
        )
        .build();

    let viewonly_row = adw::SwitchRow::builder()
        .title("View-only mode (no remote input)")
        .subtitle("Adds `-d`. Disables virtual mouse and keyboard so viewers can watch but not control.")
        .build();
    viewonly_row.set_active(s.view_only);
    group.add(&viewonly_row);

    let cursor_row = adw::SwitchRow::builder()
        .title("Render cursor into framebuffer")
        .subtitle("Adds `-r`. Burns the cursor sprite into the captured image for clients that don't draw it themselves.")
        .build();
    cursor_row.set_active(s.render_cursor);
    group.add(&cursor_row);

    let fps_row = adw::SpinRow::with_range(1.0, 120.0, 1.0);
    fps_row.set_title("Max frames per second");
    fps_row.set_subtitle("Adds `-f N` when not 30. Lower values reduce bandwidth and CPU.");
    fps_row.set_value(s.max_fps.unwrap_or(30) as f64);
    group.add(&fps_row);

    let gpu_row = adw::SwitchRow::builder()
        .title("Enable GPU features")
        .subtitle("Adds `-g`. Hardware-accelerated capture / cursor when your driver supports it.")
        .build();
    gpu_row.set_active(s.gpu);
    group.add(&gpu_row);

    let level_model = gtk::StringList::new(&[
        "Warning (default)",
        "Info",
        "Debug",
        "Trace (very noisy)",
    ]);
    let level_row = adw::ComboRow::builder()
        .title("Log level")
        .subtitle("Adds `-L`. Verbose levels help when filing bug reports; trace logs every frame.")
        .model(&level_model)
        .build();
    let initial_level: u32 = match s.log_level.as_deref() {
        Some("info") => 1,
        Some("debug") => 2,
        Some("trace") => 3,
        _ => 0,
    };
    level_row.set_selected(initial_level);
    group.add(&level_row);

    let switch_row = adw::SwitchRow::builder()
        .title("Pin to single output, disable resize")
        .subtitle("Wraps wayvnc with `-R` and `-o OUTPUT`. Useful for clients that don't handle dynamic resizes.")
        .build();
    switch_row.set_active(s.compat_mode);
    group.add(&switch_row);

    let detected = ctl::output_list().ok().unwrap_or_default();
    let hint = if detected.is_empty() {
        String::new()
    } else {
        format!(
            "Detected: {}",
            detected
                .iter()
                .map(|o| o.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let output_row = adw::EntryRow::builder()
        .title(if hint.is_empty() {
            "Output name (e.g. DP-1)".to_string()
        } else {
            hint
        })
        .show_apply_button(false)
        .build();
    if let Some(o) = &s.compat_output {
        output_row.set_text(o);
    }
    group.add(&output_row);

    let apply_row = adw::ActionRow::builder()
        .title("Apply changes")
        .subtitle("Rewrites ~/.config/systemd/user/wayvnc.service and restarts wayvnc")
        .build();
    let apply_btn = gtk::Button::with_label("Apply");
    apply_btn.add_css_class("suggested-action");
    apply_btn.set_valign(gtk::Align::Center);
    apply_row.add_suffix(&apply_btn);
    group.add(&apply_row);

    {
        let viewonly_row = viewonly_row.clone();
        let cursor_row = cursor_row.clone();
        let fps_row = fps_row.clone();
        let gpu_row = gpu_row.clone();
        let level_row = level_row.clone();
        let switch_row = switch_row.clone();
        let output_row = output_row.clone();
        let toast = toast.clone();
        apply_btn.connect_clicked(move |_| {
            let mut s = settings::Settings::load();
            s.view_only = viewonly_row.is_active();
            s.render_cursor = cursor_row.is_active();
            let fps = fps_row.value() as u32;
            s.max_fps = if fps == 30 { None } else { Some(fps) };
            s.gpu = gpu_row.is_active();
            s.log_level = match level_row.selected() {
                1 => Some("info".into()),
                2 => Some("debug".into()),
                3 => Some("trace".into()),
                _ => None,
            };
            s.compat_mode = switch_row.is_active();
            let text = output_row.text().to_string();
            s.compat_output = if text.trim().is_empty() {
                None
            } else {
                Some(text)
            };
            if let Err(e) = s.save() {
                toast.add_toast(adw::Toast::new(&format!("Save failed: {e}")));
                return;
            }
            if let Err(e) = service::install_unit(&s.wayvnc_extra_args()) {
                toast.add_toast(adw::Toast::new(&format!("Unit rewrite failed: {e}")));
                return;
            }
            if let Err(e) = service::restart() {
                toast.add_toast(adw::Toast::new(&format!(
                    "Saved, but restart failed: {e}"
                )));
                return;
            }
            toast.add_toast(adw::Toast::new("Server options applied. wayvnc restarted."));
        });
    }

    prefs.add(&group);
}

fn build_outputs_group(prefs: &adw::PreferencesPage) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Display capture")
        .description(
            "Which Wayland output (monitor) wayvnc is currently sharing. \
             Click Capture to switch — the change is live and clients stay connected.",
        )
        .build();
    prefs.add(&group);
    group
}

fn build_clients_group(prefs: &adw::PreferencesPage) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Connected clients")
        .description("Refreshes every 2 seconds.")
        .build();
    prefs.add(&group);
    group
}

fn build_raw_config_group(prefs: &adw::PreferencesPage, cfg: &config::Config) {
    let group = adw::PreferencesGroup::builder()
        .title("Raw configuration")
        .description(format!(
            "Lives in {}",
            config::config_path().display()
        ))
        .build();

    let expander = adw::ExpanderRow::builder()
        .title("Show config file (password masked)")
        .build();

    let label = gtk::Label::builder()
        .selectable(true)
        .xalign(0.0)
        .yalign(0.0)
        .wrap(false)
        .label(cfg.render_redacted())
        .build();
    label.add_css_class("monospace");

    let frame = gtk::Frame::new(None);
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_height(180)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&label)
        .build();
    frame.set_child(Some(&scroll));
    frame.set_margin_top(6);
    frame.set_margin_bottom(6);
    frame.set_margin_start(12);
    frame.set_margin_end(12);

    expander.add_row(&frame);
    group.add(&expander);
    prefs.add(&group);
}

fn build_logs_group(prefs: &adw::PreferencesPage) {
    let group = adw::PreferencesGroup::builder().title("Recent logs").build();
    let expander = adw::ExpanderRow::builder()
        .title("Show wayvnc journal (last 80 lines)")
        .build();

    let label = gtk::Label::builder()
        .selectable(true)
        .xalign(0.0)
        .yalign(0.0)
        .wrap(false)
        .build();
    label.add_css_class("monospace");

    let frame = gtk::Frame::new(None);
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_height(220)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&label)
        .build();
    frame.set_child(Some(&scroll));
    frame.set_margin_top(6);
    frame.set_margin_bottom(6);
    frame.set_margin_start(12);
    frame.set_margin_end(12);

    let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh.add_css_class("flat");
    refresh.set_valign(gtk::Align::Center);
    refresh.set_tooltip_text(Some("Reload"));
    {
        let label = label.clone();
        refresh.connect_clicked(move |_| {
            label.set_label(&service::recent_logs(80));
        });
    }
    expander.add_suffix(&refresh);

    // Populate on first expand so it doesn't run journalctl up-front for nothing.
    {
        let label = label.clone();
        expander.connect_expanded_notify(move |row| {
            if row.is_expanded() && label.label().is_empty() {
                label.set_label(&service::recent_logs(80));
            }
        });
    }

    expander.add_row(&frame);
    group.add(&expander);
    prefs.add(&group);
}

fn build_connlog_group(prefs: &adw::PreferencesPage) {
    let group = adw::PreferencesGroup::builder()
        .title("Connection log")
        .description(format!(
            "Connect and disconnect events are appended to {}",
            connlog::log_path().display()
        ))
        .build();

    let expander = adw::ExpanderRow::builder()
        .title("Show last 50 events")
        .build();

    let label = gtk::Label::builder()
        .selectable(true)
        .xalign(0.0)
        .yalign(0.0)
        .wrap(false)
        .label(connlog::tail(50))
        .build();
    label.add_css_class("monospace");

    let frame = gtk::Frame::new(None);
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_height(200)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&label)
        .build();
    frame.set_child(Some(&scroll));
    frame.set_margin_start(12);
    frame.set_margin_end(12);
    frame.set_margin_top(6);
    frame.set_margin_bottom(6);

    let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh.add_css_class("flat");
    refresh.set_valign(gtk::Align::Center);
    refresh.set_tooltip_text(Some("Reload"));
    {
        let label = label.clone();
        refresh.connect_clicked(move |_| {
            label.set_label(&connlog::tail(50));
        });
    }
    expander.add_suffix(&refresh);

    let open = gtk::Button::from_icon_name("document-open-symbolic");
    open.add_css_class("flat");
    open.set_valign(gtk::Align::Center);
    open.set_tooltip_text(Some("Open the log file in your default editor"));
    open.connect_clicked(|btn| {
        let uri = format!("file://{}", connlog::log_path().display());
        let launcher = gtk::UriLauncher::new(&uri);
        let parent_window = btn
            .root()
            .and_then(|r| r.downcast::<gtk::Window>().ok());
        launcher.launch(
            parent_window.as_ref(),
            gtk::gio::Cancellable::NONE,
            |_| {},
        );
    });
    expander.add_suffix(&open);

    // Refresh content when expanded -- file may have grown since build.
    {
        let label = label.clone();
        expander.connect_expanded_notify(move |row| {
            if row.is_expanded() {
                label.set_label(&connlog::tail(50));
            }
        });
    }

    expander.add_row(&frame);
    group.add(&expander);
    prefs.add(&group);
}

fn build_diagnostics_group(prefs: &adw::PreferencesPage) {
    let group = adw::PreferencesGroup::builder()
        .title("System diagnostics")
        .description(
            "Checks that the binaries and session features Wayhelm relies on are present. \
             Run by clicking the refresh button.",
        )
        .build();

    let rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::new(RefCell::new(Vec::new()));
    let populate = {
        let group = group.clone();
        let rows = rows.clone();
        Rc::new(move || {
            let mut h = rows.borrow_mut();
            for r in h.drain(..) {
                group.remove(&r);
            }
            for check in diagnostics::run_all() {
                let r = adw::ActionRow::builder()
                    .title(&check.name)
                    .subtitle(&check.detail)
                    .build();
                let icon = gtk::Image::from_icon_name(check.status.icon_name());
                icon.set_valign(gtk::Align::Center);
                if check.status == diagnostics::Status::Fail {
                    icon.add_css_class("error");
                } else if check.status == diagnostics::Status::Warn {
                    icon.add_css_class("warning");
                } else {
                    icon.add_css_class("success");
                }
                r.add_suffix(&icon);
                group.add(&r);
                h.push(r);
            }
        })
    };
    populate();

    let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh.add_css_class("flat");
    refresh.set_tooltip_text(Some("Re-run diagnostics"));
    {
        let populate = populate.clone();
        refresh.connect_clicked(move |_| populate());
    }
    group.set_header_suffix(Some(&refresh));

    prefs.add(&group);
}

fn build_app_prefs_group(prefs: &adw::PreferencesPage, toast: &adw::ToastOverlay) {
    let s = settings::Settings::load();
    let group = adw::PreferencesGroup::builder()
        .title("Wayhelm preferences")
        .description("Settings for the Wayhelm window itself, not wayvnc.")
        .build();

    let autostart_row = adw::SwitchRow::builder()
        .title("Start Wayhelm at login")
        .subtitle(
            "Writes ~/.config/autostart/wayhelm.desktop. Wayhelm launches with \
             --hidden so it goes straight to the tray instead of opening a window.",
        )
        .build();
    autostart_row.set_active(autostart::is_enabled());
    {
        let toast = toast.clone();
        autostart_row.connect_active_notify(move |row| {
            let r = if row.is_active() {
                autostart::enable()
            } else {
                autostart::disable()
            };
            if let Err(e) = r {
                toast.add_toast(adw::Toast::new(&format!("Autostart: {e}")));
            }
        });
    }
    group.add(&autostart_row);

    // 0 = HideToTray, 1 = Quit, 2 = Ask each time (Option None)
    let current: u32 = match s.close_action {
        Some(settings::CloseAction::HideToTray) => 0,
        Some(settings::CloseAction::Quit) => 1,
        None => 2,
    };

    let model = gtk::StringList::new(&[
        "Hide to tray",
        "Quit",
        "Ask each time",
    ]);
    let combo = adw::ComboRow::builder()
        .title("When the window is closed")
        .subtitle("Takes effect on the next close.")
        .model(&model)
        .build();
    combo.set_selected(current);
    {
        let toast = toast.clone();
        combo.connect_selected_notify(move |c| {
            let mut s = settings::Settings::load();
            s.close_action = match c.selected() {
                0 => Some(settings::CloseAction::HideToTray),
                1 => Some(settings::CloseAction::Quit),
                _ => None,
            };
            if let Err(e) = s.save() {
                toast.add_toast(adw::Toast::new(&format!("Save failed: {e}")));
            }
        });
    }
    group.add(&combo);

    prefs.add(&group);
}

fn scrolled_page(prefs: &adw::PreferencesPage) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(prefs)
        .vexpand(true)
        .build()
}

fn maybe_brackets(addr: &str) -> String {
    if addr.contains(':') {
        format!("[{addr}]")
    } else {
        addr.to_string()
    }
}

fn maybe_add_firewall_banner(
    toolbar: &adw::ToolbarView,
    toast: &adw::ToastOverlay,
    cfg: &config::Config,
    window: &adw::ApplicationWindow,
) {
    let bound = cfg.address.as_deref().unwrap_or("127.0.0.1");
    let lan_bind = bound != "127.0.0.1" && !bound.eq_ignore_ascii_case("localhost");
    if !lan_bind {
        return;
    }
    let Some(fw) = firewall::detect_active() else {
        return;
    };
    let port = cfg.port.unwrap_or(5900);

    // We can't read ufw/firewalld status without root, so we rely on a cached
    // "user said it's fine" flag: set when they open the port through us, or
    // when they explicitly dismiss the banner.
    if settings::Settings::load().firewall_opened_port == Some(port) {
        return;
    }

    let banner = adw::Banner::new(&format!(
        "{} is active and may be blocking incoming connections on TCP port {}",
        fw.label(),
        port
    ));
    banner.set_button_label(Some("Open port…"));
    banner.set_revealed(true);

    {
        let toast = toast.clone();
        let banner = banner.clone();
        let window = window.clone();
        banner.clone().connect_button_clicked(move |_| {
            ask_scope_and_open(&window, fw, port, &toast, &banner);
        });
    }
    toolbar.add_top_bar(&banner);
}

fn show_crash_dialog(window: &adw::ApplicationWindow, toast: &adw::ToastOverlay) {
    let dialog = adw::AlertDialog::builder()
        .heading("wayvnc keeps crashing")
        .body(
            "Repeated wayvnc SEGVs are an upstream bug in wayvnc/neatvnc, not in Wayhelm. \
             You can copy a diagnostic bundle for a bug report, stop the service to halt the \
             restart loop, or jump straight to the upstream issue tracker.",
        )
        .build();

    dialog.add_response("cancel", "Close");
    dialog.add_response("issue", "Open upstream tracker");
    dialog.add_response("stop", "Stop wayvnc");
    dialog.set_response_appearance("stop", adw::ResponseAppearance::Destructive);
    dialog.add_response("copy", "Copy diagnostic");
    dialog.set_response_appearance("copy", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("copy"));
    dialog.set_close_response("cancel");

    let window_for_resp = window.clone();
    let toast_for_resp = toast.clone();
    dialog.connect_response(None, move |_, response| match response {
        "copy" => {
            let text = build_diagnostic_dump();
            window_for_resp.clipboard().set_text(&text);
            toast_for_resp.add_toast(adw::Toast::new("Diagnostic copied to clipboard"));
        }
        "stop" => {
            if let Err(e) = service::stop() {
                toast_for_resp.add_toast(adw::Toast::new(&format!("Stop failed: {e}")));
            } else {
                toast_for_resp.add_toast(adw::Toast::new("wayvnc stopped"));
            }
        }
        "issue" => {
            let launcher = gtk::UriLauncher::new("https://github.com/any1/wayvnc/issues");
            launcher.launch(Some(&window_for_resp), gtk::gio::Cancellable::NONE, |_| {});
        }
        _ => {}
    });

    dialog.present(Some(window));
}

fn build_diagnostic_dump() -> String {
    use std::process::Command;
    let mut s = String::new();
    s.push_str("### Wayhelm crash diagnostic\n\n");

    s.push_str("**XDG_CURRENT_DESKTOP:** ");
    s.push_str(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "(unset)".into()));
    s.push_str("\n\n");

    s.push_str("**wayvnc --version:**\n```\n");
    if let Ok(out) = Command::new("wayvnc").arg("--version").output() {
        s.push_str(&String::from_utf8_lossy(&out.stdout));
        s.push_str(&String::from_utf8_lossy(&out.stderr));
    }
    s.push_str("```\n\n");

    s.push_str("**/etc/os-release:**\n```\n");
    if let Ok(text) = std::fs::read_to_string("/etc/os-release") {
        s.push_str(&text);
    }
    s.push_str("```\n\n");

    s.push_str("**Recent wayvnc journal (last 60 lines):**\n```\n");
    s.push_str(&service::recent_logs(60));
    s.push_str("```\n");

    s
}

fn mark_firewall_handled(port: u16) {
    let mut s = settings::Settings::load();
    s.firewall_opened_port = Some(port);
    let _ = s.save();
}

fn ask_scope_and_open(
    window: &adw::ApplicationWindow,
    fw: firewall::Firewall,
    port: u16,
    toast: &adw::ToastOverlay,
    banner: &adw::Banner,
) {
    let cidr = firewall::detect_primary_lan_cidr();

    let dialog = adw::AlertDialog::builder()
        .heading("Open firewall port")
        .body(format!(
            "Add a rule to {} for TCP port {}. Choose which sources are allowed.",
            fw.label(),
            port
        ))
        .build();

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("dismiss", "Don't show again");
    if let Some(cidr) = &cidr {
        dialog.add_response("lan", &format!("LAN only ({cidr})"));
        dialog.set_response_appearance("lan", adw::ResponseAppearance::Suggested);
    }
    dialog.add_response("any", "Any source");
    dialog.set_default_response(Some(if cidr.is_some() { "lan" } else { "any" }));
    dialog.set_close_response("cancel");

    {
        let toast = toast.clone();
        let banner = banner.clone();
        let cidr = cidr.clone();
        dialog.connect_response(None, move |_, response| {
            match response {
                "dismiss" => {
                    mark_firewall_handled(port);
                    banner.set_revealed(false);
                    toast.add_toast(adw::Toast::new(
                        "Banner dismissed — re-shows if you change ports",
                    ));
                    return;
                }
                "cancel" => return,
                _ => {}
            }
            let scope = match response {
                "lan" => match &cidr {
                    Some(c) => firewall::Scope::Lan(c.clone()),
                    None => return,
                },
                "any" => firewall::Scope::Any,
                _ => return,
            };
            match firewall::open_port(fw, port, &scope) {
                Ok(()) => {
                    mark_firewall_handled(port);
                    banner.set_revealed(false);
                    toast.add_toast(adw::Toast::new("Firewall rule added"));
                }
                Err(e) => {
                    toast.add_toast(adw::Toast::new(&format!("Firewall: {e}")));
                }
            }
        });
    }

    dialog.present(Some(window));
}
