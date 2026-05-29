use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::{app as appmod, certs, config, netinfo, service, util};

#[derive(Clone)]
struct State {
    bind_lan: bool,
    port: u16,
    username: String,
    password: String,
    cert_cn: String,
    cert_days: u32,
    install_service: bool,
    enable_on_login: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            bind_lan: false,
            port: 5900,
            username: "vnc".into(),
            password: util::random_password(16),
            cert_cn: netinfo::hostname(),
            cert_days: 825,
            install_service: true,
            enable_on_login: true,
        }
    }
}

pub fn build(window: &adw::ApplicationWindow) -> gtk::Widget {
    let state = Rc::new(RefCell::new(State::default()));
    let nav = adw::NavigationView::new();

    nav.add(&page_welcome(&nav));
    nav.add(&page_network(&nav, state.clone()));
    nav.add(&page_auth(&nav, state.clone()));
    nav.add(&page_tls(&nav, state.clone()));
    nav.add(&page_service(&nav, state.clone()));
    nav.add(&page_summary(state.clone(), window.clone()));

    nav.upcast()
}

// ---------------------------------------------------------------------------
// Page helpers
// ---------------------------------------------------------------------------

fn page_wrap(
    tag: &str,
    title: &str,
    body: gtk::Widget,
    next_btn: Option<&gtk::Button>,
) -> adw::NavigationPage {
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    if let Some(b) = next_btn {
        header.pack_end(b);
    }
    toolbar.add_top_bar(&header);

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&body)
        .vexpand(true)
        .build();
    toolbar.set_content(Some(&scrolled));

    adw::NavigationPage::builder()
        .tag(tag)
        .title(title)
        .child(&toolbar)
        .build()
}

fn next_button(label: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.add_css_class("suggested-action");
    b
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

fn page_welcome(nav: &adw::NavigationView) -> adw::NavigationPage {
    let status = adw::StatusPage::builder()
        .icon_name("network-server-symbolic")
        .title("Welcome to Wayhelm")
        .description(
            "Wayhelm sets up wayvnc so you can reach this desktop from another machine over an encrypted, password-protected connection.\n\n\
             We'll choose where to listen, set a username and password, generate a TLS certificate and an RSA key, and install a systemd user service.\n\n\
             Nothing on your system changes until the final step.",
        )
        .build();

    let start = next_button("Get started");
    start.add_css_class("pill");
    start.set_halign(gtk::Align::Center);
    {
        let nav = nav.clone();
        start.connect_clicked(move |_| {
            nav.push_by_tag("network");
        });
    }

    let bx = gtk::Box::new(gtk::Orientation::Vertical, 18);
    bx.append(&status);
    bx.append(&start);
    bx.set_margin_bottom(24);

    page_wrap("welcome", "Welcome", bx.upcast(), None)
}

fn page_network(nav: &adw::NavigationView, state: Rc<RefCell<State>>) -> adw::NavigationPage {
    let prefs = adw::PreferencesPage::new();

    let bind_group = adw::PreferencesGroup::builder()
        .title("Where should wayvnc listen?")
        .description(
            "Loopback is the safest default — only this machine can reach it. \
             You can still log in from elsewhere by forwarding the port over SSH:\n\
             ssh -L 5900:127.0.0.1:5900 you@this-host\n\n\
             Choose 'Any network interface' if you want to connect to it directly from the LAN.",
        )
        .build();

    let loopback_row = adw::ActionRow::builder()
        .title("Loopback only (127.0.0.1)")
        .subtitle("Reachable from this machine, or via an SSH tunnel")
        .activatable(true)
        .build();
    let loopback_check = gtk::CheckButton::new();
    loopback_check.set_valign(gtk::Align::Center);
    loopback_row.add_prefix(&loopback_check);
    loopback_row.set_activatable_widget(Some(&loopback_check));

    let lan_row = adw::ActionRow::builder()
        .title("Any network interface (0.0.0.0)")
        .subtitle("Reachable from any device that can route to this machine")
        .activatable(true)
        .build();
    let lan_check = gtk::CheckButton::new();
    lan_check.set_valign(gtk::Align::Center);
    lan_check.set_group(Some(&loopback_check));
    lan_row.add_prefix(&lan_check);
    lan_row.set_activatable_widget(Some(&lan_check));

    if state.borrow().bind_lan {
        lan_check.set_active(true);
    } else {
        loopback_check.set_active(true);
    }
    {
        let s = state.clone();
        lan_check.connect_toggled(move |c| {
            s.borrow_mut().bind_lan = c.is_active();
        });
    }

    bind_group.add(&loopback_row);
    bind_group.add(&lan_row);
    prefs.add(&bind_group);

    let port_group = adw::PreferencesGroup::new();
    let port_row = adw::SpinRow::with_range(1024.0, 65535.0, 1.0);
    port_row.set_title("TCP port");
    port_row.set_subtitle("Default 5900 (the well-known VNC port)");
    port_row.set_value(state.borrow().port as f64);
    {
        let s = state.clone();
        port_row.connect_value_notify(move |w| {
            s.borrow_mut().port = w.value() as u16;
        });
    }
    port_group.add(&port_row);
    prefs.add(&port_group);

    let iface_group = adw::PreferencesGroup::builder()
        .title("Detected network addresses")
        .description("For reference — these are the IPs you'd connect to if you choose LAN.")
        .build();
    let addrs = netinfo::local_addresses();
    if addrs.is_empty() {
        let r = adw::ActionRow::builder()
            .title("No non-loopback addresses detected")
            .build();
        iface_group.add(&r);
    } else {
        for iface in addrs {
            let r = adw::ActionRow::builder()
                .title(iface.addr.to_string())
                .subtitle(iface.name)
                .build();
            iface_group.add(&r);
        }
    }
    prefs.add(&iface_group);

    let next = next_button("Next");
    {
        let nav = nav.clone();
        next.connect_clicked(move |_| {
            nav.push_by_tag("auth");
        });
    }
    page_wrap("network", "Network", prefs.upcast(), Some(&next))
}

fn page_auth(nav: &adw::NavigationView, state: Rc<RefCell<State>>) -> adw::NavigationPage {
    let prefs = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("Authentication")
        .description(
            "Wayvnc requires both a username and a password when authentication is on. \
             We've pre-filled a strong random password — copy it somewhere safe before continuing.",
        )
        .build();

    let user = adw::EntryRow::builder().title("Username").build();
    user.set_text(&state.borrow().username);
    {
        let s = state.clone();
        user.connect_changed(move |w| {
            s.borrow_mut().username = w.text().to_string();
        });
    }
    group.add(&user);

    let pass = adw::PasswordEntryRow::builder().title("Password").build();
    pass.set_text(&state.borrow().password);
    pass.set_show_apply_button(false);
    {
        let s = state.clone();
        pass.connect_changed(move |w| {
            s.borrow_mut().password = w.text().to_string();
        });
    }

    let regen = gtk::Button::from_icon_name("view-refresh-symbolic");
    regen.set_tooltip_text(Some("Generate a new strong password"));
    regen.add_css_class("flat");
    regen.set_valign(gtk::Align::Center);
    {
        let pass = pass.clone();
        regen.connect_clicked(move |_| {
            pass.set_text(&util::random_password(16));
        });
    }
    pass.add_suffix(&regen);
    group.add(&pass);
    prefs.add(&group);

    let next = next_button("Next");
    {
        let nav = nav.clone();
        next.connect_clicked(move |_| {
            nav.push_by_tag("tls");
        });
    }
    page_wrap("auth", "Authentication", prefs.upcast(), Some(&next))
}

fn page_tls(nav: &adw::NavigationView, state: Rc<RefCell<State>>) -> adw::NavigationPage {
    let prefs = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("TLS certificate")
        .description(
            "Wayhelm will generate a self-signed TLS certificate and a separate RSA key (used by wayvnc's RSA-AES auth path). \
             Self-signed means your VNC client will warn you on the first connection — verify the fingerprint shown on the dashboard, then trust it. \
             Files are written to ~/.config/wayvnc/ with 600 permissions.",
        )
        .build();

    let cn = adw::EntryRow::builder().title("Common name (CN)").build();
    cn.set_text(&state.borrow().cert_cn);
    {
        let s = state.clone();
        cn.connect_changed(move |w| {
            s.borrow_mut().cert_cn = w.text().to_string();
        });
    }
    group.add(&cn);

    let days = adw::SpinRow::with_range(30.0, 3650.0, 1.0);
    days.set_title("Validity (days)");
    days.set_subtitle("Apple/Safari clients reject TLS certs valid for more than 825 days");
    days.set_value(state.borrow().cert_days as f64);
    {
        let s = state.clone();
        days.connect_value_notify(move |w| {
            s.borrow_mut().cert_days = w.value() as u32;
        });
    }
    group.add(&days);
    prefs.add(&group);

    let next = next_button("Next");
    {
        let nav = nav.clone();
        next.connect_clicked(move |_| {
            nav.push_by_tag("service");
        });
    }
    page_wrap("tls", "TLS certificate", prefs.upcast(), Some(&next))
}

fn page_service(nav: &adw::NavigationView, state: Rc<RefCell<State>>) -> adw::NavigationPage {
    let prefs = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("Systemd service")
        .description(
            "The wayvnc package ships a system unit that hard-requires sway.service. \
             Wayhelm installs a compositor-agnostic copy at ~/.config/systemd/user/wayvnc.service \
             so it works on Hyprland, river, sway, and other wlroots compositors.",
        )
        .build();

    let install = adw::SwitchRow::builder()
        .title("Install user service")
        .subtitle("Manage wayvnc via systemctl --user start/stop/enable")
        .build();
    install.set_active(state.borrow().install_service);
    {
        let s = state.clone();
        install.connect_active_notify(move |row| {
            s.borrow_mut().install_service = row.is_active();
        });
    }
    group.add(&install);

    let autostart = adw::SwitchRow::builder()
        .title("Start automatically on login")
        .subtitle("Enables the service so it launches with your graphical session")
        .build();
    autostart.set_active(state.borrow().enable_on_login);
    {
        let s = state.clone();
        autostart.connect_active_notify(move |row| {
            s.borrow_mut().enable_on_login = row.is_active();
        });
    }
    group.add(&autostart);

    prefs.add(&group);

    let next = next_button("Review");
    {
        let nav = nav.clone();
        next.connect_clicked(move |_| {
            nav.push_by_tag("summary");
        });
    }
    page_wrap("service", "Service", prefs.upcast(), Some(&next))
}

fn page_summary(state: Rc<RefCell<State>>, window: adw::ApplicationWindow) -> adw::NavigationPage {
    let preview = gtk::Label::builder()
        .selectable(true)
        .xalign(0.0)
        .yalign(0.0)
        .wrap(false)
        .build();
    preview.add_css_class("monospace");

    let preview_frame = gtk::Frame::new(None);
    let preview_scroll = gtk::ScrolledWindow::builder()
        .min_content_height(220)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&preview)
        .build();
    preview_frame.set_child(Some(&preview_scroll));
    preview_frame.set_margin_top(6);
    preview_frame.set_margin_bottom(6);

    let preview_group = adw::PreferencesGroup::builder()
        .title("Configuration preview")
        .description("This will be written to ~/.config/wayvnc/config (password masked).")
        .build();
    preview_group.add(&preview_frame);

    let banner = adw::Banner::builder()
        .title("Self-signed certs cause a warning on first connect — accept once, pinned thereafter.")
        .revealed(true)
        .build();

    let apply = gtk::Button::with_label("Apply & start wayvnc");
    apply.add_css_class("suggested-action");
    apply.add_css_class("pill");
    apply.set_halign(gtk::Align::Center);

    let error = gtk::Label::new(None);
    error.add_css_class("error");
    error.set_wrap(true);
    error.set_xalign(0.0);

    let action_group = adw::PreferencesGroup::new();
    let action_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
    action_box.append(&banner);
    action_box.append(&apply);
    action_box.append(&error);
    action_group.add(&action_box);

    let prefs = adw::PreferencesPage::new();
    prefs.add(&preview_group);
    prefs.add(&action_group);

    let page = page_wrap("summary", "Review and apply", prefs.upcast(), None);

    // Refresh the preview each time the user lands on this page (they may
    // have gone back and tweaked something).
    {
        let state = state.clone();
        let preview = preview.clone();
        page.connect_showing(move |_| {
            let cfg = build_config(&state.borrow());
            preview.set_label(&cfg.render_redacted());
        });
    }
    // Also populate immediately in case the signal hasn't fired yet.
    preview.set_label(&build_config(&state.borrow()).render_redacted());

    {
        let state = state.clone();
        let window = window.clone();
        let error = error.clone();
        apply.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            error.set_label("");
            match apply_setup(&state.borrow()) {
                Ok(()) => {
                    appmod::show_dashboard(&window);
                }
                Err(e) => {
                    error.set_label(&format!("Setup failed: {e:#}"));
                    btn.set_sensitive(true);
                }
            }
        });
    }

    page
}

// ---------------------------------------------------------------------------
// Apply
// ---------------------------------------------------------------------------

fn build_config(s: &State) -> config::Config {
    let mut c = config::Config::default();
    c.address = Some(
        if s.bind_lan {
            "0.0.0.0".into()
        } else {
            "127.0.0.1".into()
        },
    );
    c.port = Some(s.port);
    c.enable_auth = true;
    c.username = Some(s.username.clone());
    c.password = Some(s.password.clone());
    let paths = certs::CertPaths::default_paths();
    c.certificate_file = Some(paths.cert);
    c.private_key_file = Some(paths.key);
    c.rsa_private_key_file = Some(paths.rsa_key);
    c
}

fn apply_setup(s: &State) -> anyhow::Result<()> {
    certs::generate(&s.cert_cn, s.cert_days)?;
    build_config(s).save()?;
    if s.install_service {
        // Wizard always installs the bare unit; compat-mode / output pinning
        // happen later from the dashboard.
        service::install_unit("")?;
        if s.enable_on_login {
            service::enable()?;
        }
        service::start()?;
    }
    Ok(())
}
