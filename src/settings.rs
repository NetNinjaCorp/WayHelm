use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloseAction {
    HideToTray,
    Quit,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// What the X button does. None means "ask the first time".
    pub close_action: Option<CloseAction>,
    /// Bake `-R` (no framebuffer resize) and `-o <output>` into the systemd
    /// unit's ExecStart so legacy/picky VNC clients see a fixed display.
    pub compat_mode: bool,
    pub compat_output: Option<String>,
    /// `-d`: disable virtual mouse/keyboard so remote viewers can watch but
    /// not interact.
    pub view_only: bool,
    /// `-r`: render the cursor sprite into the framebuffer, for clients that
    /// don't draw the cursor themselves.
    pub render_cursor: bool,
    /// `-f`: frame rate cap. `None` = wayvnc default (30).
    pub max_fps: Option<u32>,
    /// `-L`: log level for wayvnc. `None` = warning (wayvnc default).
    /// Valid values: "info", "debug", "trace".
    pub log_level: Option<String>,
    /// `-g`: enable GPU-accelerated features (hardware cursor etc.).
    pub gpu: bool,
    /// Last port for which the user opened the firewall through us (or said
    /// "stop bothering me"). Used to suppress the firewall banner without
    /// requiring root to actually query ufw/firewalld -- those status calls
    /// need privileges Wayhelm doesn't hold.
    pub firewall_opened_port: Option<u16>,
}

pub fn settings_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("wayhelm")
}

pub fn settings_path() -> PathBuf {
    settings_dir().join("settings.json")
}

impl Settings {
    pub fn load() -> Self {
        fs::read_to_string(settings_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let dir = settings_dir();
        fs::create_dir_all(&dir).context("creating wayhelm config dir")?;
        let json = serde_json::to_string_pretty(self).context("serializing settings")?;
        let path = settings_path();
        fs::write(&path, json).context("writing settings.json")?;
        // 0600 — settings may grow to hold non-secret-but-private state
        // (close-behavior choice, dismissed-banner flags). Better hygienic
        // than relying on umask defaults.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .context("chmod 600 settings.json")?;
        }
        Ok(())
    }

    /// Build the extra arguments wayvnc should be launched with, given the
    /// current settings. Empty string when no flags apply.
    pub fn wayvnc_extra_args(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.compat_mode {
            parts.push("-R".into());
            // Only pass `-o` when the saved output name passes the strict
            // whitelist. Anything else would risk injecting newlines or
            // shell metacharacters into the systemd unit's ExecStart line.
            if let Some(out) = self
                .compat_output
                .as_deref()
                .and_then(safe_output_name)
            {
                parts.push("-o".into());
                parts.push(out);
            }
        }
        if self.view_only {
            parts.push("-d".into());
        }
        if self.render_cursor {
            parts.push("-r".into());
        }
        if let Some(fps) = self.max_fps {
            // Skip when it matches wayvnc's built-in default.
            if fps != 30 {
                parts.push("-f".into());
                parts.push(fps.to_string());
            }
        }
        if let Some(level) = self.log_level.as_deref().filter(|l| *l != "warning") {
            parts.push("-L".into());
            parts.push(level.to_string());
        }
        if self.gpu {
            parts.push("-g".into());
        }
        parts.join(" ")
    }
}

/// Wayland output identifiers are well-defined (e.g. DP-1, HDMI-A-1, eDP-1,
/// Virtual-1). Restrict to that shape so a maliciously crafted value can't
/// land arbitrary text in our generated systemd ExecStart line.
pub fn safe_output_name(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}
