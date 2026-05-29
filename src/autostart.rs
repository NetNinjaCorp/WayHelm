use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

/// XDG autostart: every modern DE looks at ~/.config/autostart/ on login and
/// launches any `*.desktop` file found there. Honored by GNOME, KDE, Hyprland
/// (via xdg-desktop-portal-hyprland), sway, XFCE, MATE, Cinnamon, etc.
pub fn autostart_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("autostart/wayhelm.desktop")
}

const AUTOSTART_DESKTOP: &str = "[Desktop Entry]
Type=Application
Name=Wayhelm
GenericName=VNC Server Configuration
Comment=Manage the wayvnc Wayland VNC server (autostart)
Exec=wayhelm --hidden
Icon=network-server
Terminal=false
Categories=System;Settings;Network;
X-GNOME-Autostart-enabled=true
StartupNotify=false
";

pub fn is_enabled() -> bool {
    autostart_path().exists()
}

pub fn enable() -> Result<()> {
    let path = autostart_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("creating ~/.config/autostart")?;
    }
    fs::write(&path, AUTOSTART_DESKTOP)
        .with_context(|| format!("writing {}", path.display()))
}

pub fn disable() -> Result<()> {
    let path = autostart_path();
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("removing {}", path.display()))?;
    }
    Ok(())
}
