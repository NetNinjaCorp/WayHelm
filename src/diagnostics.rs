use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    pub fn icon_name(self) -> &'static str {
        match self {
            Status::Ok => "emblem-ok-symbolic",
            Status::Warn => "dialog-warning-symbolic",
            Status::Fail => "dialog-error-symbolic",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Check {
    pub name: String,
    pub status: Status,
    pub detail: String,
}

/// Run all pre-req checks. Cheap; safe to call on every dashboard rebuild.
pub fn run_all() -> Vec<Check> {
    vec![
        check_binary(
            "wayvnc",
            "wayvnc",
            Severity::Fail,
            "The VNC server itself. Install via your package manager (e.g. `pacman -S wayvnc` on Arch).",
        ),
        check_binary(
            "wayvncctl",
            "wayvncctl",
            Severity::Warn,
            "Live state (client list, output switching) needs this. Usually ships with wayvnc.",
        ),
        check_binary(
            "openssl",
            "openssl",
            Severity::Fail,
            "Required to generate the TLS certificate and RSA key during setup.",
        ),
        check_binary(
            "systemctl",
            "systemctl",
            Severity::Fail,
            "Required to manage the wayvnc user service.",
        ),
        check_binary(
            "journalctl",
            "journalctl",
            Severity::Warn,
            "Used to display the wayvnc log tail in the Advanced tab.",
        ),
        check_binary(
            "ip",
            "ip",
            Severity::Fail,
            "Required to enumerate LAN addresses for the Connecting card and firewall scope.",
        ),
        check_binary(
            "pkexec",
            "pkexec",
            Severity::Warn,
            "Used to add firewall rules without dropping you into a terminal. Without it, you'll need to run `sudo ufw allow ...` manually.",
        ),
        check_wayland(),
        check_wlroots_compositor(),
        check_sni_host(),
    ]
}

enum Severity {
    Fail,
    Warn,
}

fn check_binary(name: &str, cmd: &str, sev: Severity, help: &str) -> Check {
    match find_in_path(cmd) {
        Some(path) => Check {
            name: name.into(),
            status: Status::Ok,
            detail: format!("Found at {}", path.display()),
        },
        None => Check {
            name: name.into(),
            status: match sev {
                Severity::Fail => Status::Fail,
                Severity::Warn => Status::Warn,
            },
            detail: help.into(),
        },
    }
}

fn find_in_path(cmd: &str) -> Option<PathBuf> {
    if cmd.contains('/') {
        let p = PathBuf::from(cmd);
        return p.exists().then_some(p);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(cmd))
        .find(|p| p.exists())
}

fn check_wayland() -> Check {
    match std::env::var("WAYLAND_DISPLAY") {
        Ok(d) if !d.is_empty() => Check {
            name: "Wayland session".into(),
            status: Status::Ok,
            detail: format!("WAYLAND_DISPLAY={d}"),
        },
        _ => Check {
            name: "Wayland session".into(),
            status: Status::Fail,
            detail: "WAYLAND_DISPLAY isn't set. Wayvnc has to attach to a running Wayland compositor.".into(),
        },
    }
}

fn check_wlroots_compositor() -> Check {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    const WLROOTS: &[&str] = &["Hyprland", "sway", "river", "wayfire", "labwc"];
    if WLROOTS.iter().any(|w| desktop.eq_ignore_ascii_case(w)) {
        Check {
            name: "Compositor".into(),
            status: Status::Ok,
            detail: format!("{desktop} — wlroots-based, wayvnc-compatible."),
        }
    } else {
        Check {
            name: "Compositor".into(),
            status: Status::Warn,
            detail: format!(
                "XDG_CURRENT_DESKTOP={} — wayvnc requires a wlroots-based compositor \
                 (Hyprland, sway, river, wayfire, labwc). It may not work here.",
                if desktop.is_empty() { "(unset)" } else { &desktop }
            ),
        }
    }
}

fn check_sni_host() -> Check {
    let listed = Command::new("busctl")
        .args(["--user", "list", "--no-pager"])
        .output();
    let has_watcher = matches!(listed, Ok(o) if o.status.success()
        && String::from_utf8_lossy(&o.stdout).contains("StatusNotifierWatcher"));
    if has_watcher {
        Check {
            name: "Tray host (StatusNotifier)".into(),
            status: Status::Ok,
            detail: "Found a StatusNotifierWatcher on the session bus — your bar should display Wayhelm's tray icon.".into(),
        }
    } else {
        Check {
            name: "Tray host (StatusNotifier)".into(),
            status: Status::Warn,
            detail: "No StatusNotifierWatcher on the session bus. The tray icon won't appear. (waybar with the `tray` module, swaync, or eww-tray would provide this.)".into(),
        }
    }
}
