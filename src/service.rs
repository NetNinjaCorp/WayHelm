use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, Context, Result};

pub const UNIT_NAME: &str = "wayvnc.service";

pub fn user_unit_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("systemd/user")
}

pub fn user_unit_path() -> PathBuf {
    user_unit_dir().join(UNIT_NAME)
}

/// Render a wlroots-compositor-agnostic user unit. The distro-shipped unit
/// at /usr/lib/systemd/user/wayvnc.service hard-requires sway.service, which
/// breaks on Hyprland, river, etc. Installing our own copy at
/// ~/.config/systemd/user/ overrides the system one for the current user.
///
/// `extra_args` is appended verbatim to `/usr/bin/wayvnc` in ExecStart.
pub fn render_unit(extra_args: &str) -> String {
    let trimmed = extra_args.trim();
    let exec_start = if trimmed.is_empty() {
        "/usr/bin/wayvnc".to_string()
    } else {
        format!("/usr/bin/wayvnc {trimmed}")
    };
    format!(
        "[Unit]
Description=A VNC server for wlroots based Wayland compositors (managed by Wayhelm)
PartOf=graphical-session.target
After=graphical-session.target

[Service]
Type=simple
ExecStart={exec_start}
Restart=on-failure
RestartSec=2
TimeoutStopSec=10

[Install]
WantedBy=graphical-session.target
"
    )
}

pub fn install_unit(extra_args: &str) -> Result<()> {
    let dir = user_unit_dir();
    std::fs::create_dir_all(&dir).context("creating ~/.config/systemd/user")?;
    std::fs::write(user_unit_path(), render_unit(extra_args))
        .context("writing wayvnc.service unit")?;
    systemctl(&["daemon-reload"])?;
    Ok(())
}

pub fn start() -> Result<()> {
    // reset-failed clears prior start-limit-hit so a flapping unit doesn't
    // lock the user out of trying again. Harmless when the unit isn't failed.
    let _ = systemctl(&["reset-failed", UNIT_NAME]);
    systemctl(&["start", UNIT_NAME])
}

pub fn stop() -> Result<()> {
    systemctl(&["stop", UNIT_NAME])
}

pub fn restart() -> Result<()> {
    let _ = systemctl(&["reset-failed", UNIT_NAME]);
    systemctl(&["restart", UNIT_NAME])
}

pub fn enable() -> Result<()> {
    systemctl(&["enable", UNIT_NAME])
}

pub fn disable() -> Result<()> {
    systemctl(&["disable", UNIT_NAME])
}

#[derive(Debug, Clone, Default)]
pub struct ServiceStatus {
    pub installed: bool,
    pub active: bool,
    pub enabled: bool,
    pub sub_state: String,
}

pub fn status() -> ServiceStatus {
    ServiceStatus {
        installed: user_unit_path().exists(),
        active: systemctl_value(&["is-active", UNIT_NAME])
            .map(|s| s.trim() == "active")
            .unwrap_or(false),
        enabled: systemctl_value(&["is-enabled", UNIT_NAME])
            .map(|s| s.trim() == "enabled")
            .unwrap_or(false),
        sub_state: systemctl_value(&["show", UNIT_NAME, "-p", "SubState", "--value"])
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

pub fn recent_logs(lines: u32) -> String {
    Command::new("journalctl")
        .args([
            "--user",
            "-u",
            UNIT_NAME,
            "-n",
            &lines.to_string(),
            "--no-pager",
        ])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Count crash-ish events in the wayvnc journal within the trailing window.
/// We match the signatures systemd prints when wayvnc dies (SEGV core dumps
/// and non-zero exits), so a wayvnc that quits cleanly doesn't count.
pub fn recent_crash_count(minutes: u32) -> u32 {
    let out = Command::new("journalctl")
        .args([
            "--user",
            "-u",
            UNIT_NAME,
            "--since",
            &format!("{minutes} minutes ago"),
            "--no-pager",
        ])
        .output();
    let Ok(out) = out else {
        return 0;
    };
    if !out.status.success() {
        return 0;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| {
            l.contains("status=11/SEGV")
                || l.contains("code=dumped")
                || l.contains("Failed with result 'core-dump'")
        })
        .count() as u32
}

fn systemctl(args: &[&str]) -> Result<()> {
    let out = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .context("spawning systemctl")?;
    if !out.status.success() {
        return Err(anyhow!(
            "systemctl --user {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

fn systemctl_value(args: &[&str]) -> Result<String> {
    // is-active / is-enabled exit non-zero when the answer is "no" — we want
    // the stdout regardless, so don't check the exit code here.
    let out = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .context("spawning systemctl")?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
