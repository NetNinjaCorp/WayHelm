# Wayhelm

A GTK4 + libadwaita graphical front-end for [**wayvnc**](https://github.com/any1/wayvnc), the VNC server for wlroots-based Wayland compositors (Hyprland, sway, river, wayfire, labwc).

Wayhelm handles the parts of running wayvnc that aren't obvious: secure first-time setup, TLS / RSA key generation, the systemd user service, firewall rules, monitor selection, and live status — so you can offer remote access to your Wayland desktop without piecing together a half-dozen tutorials.

## What's in the box

**Setup wizard** — first launch walks through binding (loopback or LAN), username + auto-generated 16-char password, TLS + RSA cert generation, and installs a wlroots-compatible systemd `--user` unit (the distro-shipped one hard-requires `sway.service`, which breaks every other compositor).

**Dashboard** (three tabs):

- **Overview** — service status, live start/stop controls, copyable `vnc://` URLs for every LAN interface, connected-client list with disconnect, live monitor switcher.
- **Settings** — 3-way auth mode (encryption required / encrypted fallbacks / disabled), credentials, TLS cert fingerprint + regen, keyboard layout (`xkb_*`), wayvnc CLI options (`-d`, `-r`, `-f`, `-R`, `-o`, `-L`, `-g`), Wayhelm app preferences.
- **Advanced** — pre-flight system diagnostics, connection log viewer, raw config viewer, wayvnc journal tail.

**Tray icon** — sits in your bar via StatusNotifierItem (waybar `tray` module, swaync, Plasma…). Grey when idle, green when a client is connected. Tooltip lists active clients. Right-click for Show / Start-Stop / Switch output ▶ / Quit.

**Quality-of-life**:
- Firewall (ufw / firewalld) detection with one-click `pkexec` rule add, scoped to LAN-only by default
- Crash visibility — banner appears when wayvnc segfaults repeatedly, with a one-button diagnostic-bundle copy for upstream issues
- Connection log of connect/disconnect events with duration
- Close-to-tray with configurable behavior
- XDG autostart toggle (uses `wayhelm --hidden` for tray-only launch)

## Requirements

**Runtime:**
- A wlroots-based Wayland compositor (Hyprland, sway, river, wayfire, labwc)
- `wayvnc` + `wayvncctl` (the thing being managed)
- `openssl`, `systemctl`, `journalctl`, `ip`
- `gtk4`, `libadwaita`, `dbus` shared libraries
- A StatusNotifierItem-aware tray host (waybar with `tray` module, swaync, plasma-style trays) — for the tray icon to be visible

**Optional but recommended:**
- `polkit` (for `pkexec` firewall rule changes)
- `ufw` or `firewalld` (managed firewall backends)

## Install

### Arch / CachyOS / Manjaro

Once published to AUR:
```sh
yay -S wayhelm        # stable
yay -S wayhelm-git    # latest commit
```

Until then, build the PKGBUILD locally:
```sh
git clone https://github.com/NetNinjaCorp/WayHelm.git
cd WayHelm/packaging
makepkg -si
```

### NixOS / nix-darwin

```sh
nix run github:NetNinjaCorp/WayHelm
# or add to your inputs and use packages.x86_64-linux.default
```

A dev shell is also exposed:
```sh
nix develop github:NetNinjaCorp/WayHelm
```

### Any other distro

`install.sh` auto-detects pacman / dnf / apt / zypper, installs build + runtime deps, builds, installs to `/usr/local`:
```sh
git clone https://github.com/NetNinjaCorp/WayHelm.git
cd WayHelm
./install.sh                   # → /usr/local
PREFIX=/usr ./install.sh       # → /usr
```

Or do it by hand with `make`:
```sh
make build
sudo make install PREFIX=/usr
```

Build prerequisites (Arch package names; substitute your distro's equivalents): `rust`, `cargo`, `gtk4`, `libadwaita`, `dbus`, `pkgconf`.

## Usage

First launch shows the **setup wizard** if there's no `~/.config/wayvnc/config`. Walk through the six steps and click **Apply & start wayvnc** on the review page — wayvnc starts, the dashboard opens, and the tray icon appears (assuming your bar supports StatusNotifier).

Subsequent launches go straight to the dashboard. Connect from any VNC client to one of the URLs shown on the **Overview** tab.

### Files

| Path | Purpose |
|------|---------|
| `~/.config/wayvnc/config` | wayvnc's own config (auth, TLS, keyboard) |
| `~/.config/wayvnc/{tls_cert,tls_key,rsa_key}.pem` | generated certs and key |
| `~/.config/systemd/user/wayvnc.service` | the systemd user unit we install |
| `~/.config/wayhelm/settings.json` | Wayhelm's own preferences |
| `~/.config/autostart/wayhelm.desktop` | XDG autostart entry (when enabled) |
| `~/.local/state/wayhelm/connections.log` | connect/disconnect events |

## Client compatibility (and a warning)

Wayvnc + neatvnc enforce encrypted security types when authentication is enabled. As of wayvnc 0.9.x + neatvnc 0.9.x:

- **TigerVNC / Remmina / RealVNC viewer** — speak VeNCrypt-TLS, can connect with default secure setup. Note: **RealVNC viewer reproducibly crashes wayvnc** on connect (libaml threading bug, upstream issue, *not* caused by Wayhelm). TigerVNC and Remmina are recommended.
- **TightVNC / UltraVNC** — only speak legacy VNC Auth, which wayvnc cannot offer without disabling auth entirely (neatvnc requires `ALLOW_BROKEN_CRYPTO` + `!REQUIRE_USERNAME`, neither of which wayvnc exposes). Set **Authentication mode → Disabled** in Settings to use these viewers, and rely on the firewall to constrain who can connect.

Wayhelm surfaces these compatibility paths explicitly so you can pick a working combination quickly.

## Reboot-and-reconnect

By design, the wayvnc unit waits for `graphical-session.target` — it can't capture a Wayland desktop that doesn't exist. For unattended remote access after a reboot, enable autologin in your display manager (e.g., LightDM `autologin-user=`) and ensure user lingering is on (`loginctl enable-linger $USER`). The tradeoff is that physical access yields an unlocked desktop.

## Building from source

```sh
cargo build --release
./target/release/wayhelm
```

CI builds release binaries on every tag push (see `.github/workflows/release.yml`); attached to the GitHub release.

## License

MIT. See `LICENSE`.

## Project status

v0.1.0 — initial public release. Filed under "scratching the author's own itch" and known to work on Arch + Hyprland with wayvnc 0.9.1. Bug reports and PRs welcome.
