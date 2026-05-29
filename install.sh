#!/usr/bin/env bash
# Wayhelm one-shot installer for Linux distros without a native package.
#
# Detects the package manager, installs build + runtime dependencies, builds
# Wayhelm with `cargo build --release`, and installs to $PREFIX (default
# /usr/local).
#
# Environment:
#   PREFIX    - install prefix (default /usr/local)
#   SKIP_DEPS - set to 1 to skip the dependency install step
#
# Usage:
#   ./install.sh                      # build + install to /usr/local
#   PREFIX=/usr ./install.sh          # install to /usr (distro convention)
#   SKIP_DEPS=1 ./install.sh          # if you've already installed the deps

set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"

say()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31m==>\033[0m %s\n' "$*" >&2; exit 1; }

# ---------- detect package manager ----------
if command -v pacman   >/dev/null 2>&1; then PM=pacman
elif command -v dnf    >/dev/null 2>&1; then PM=dnf
elif command -v apt-get>/dev/null 2>&1; then PM=apt
elif command -v zypper >/dev/null 2>&1; then PM=zypper
else
  PM=
  warn "No supported package manager detected — you'll need to install gtk4, libadwaita, dbus, pkgconf, cargo, and wayvnc yourself."
fi

install_deps() {
  [ -z "$PM" ] && return 0
  say "Installing build + runtime dependencies via $PM..."
  case "$PM" in
    pacman)
      sudo pacman -S --needed --noconfirm \
        gtk4 libadwaita dbus pkgconf rust cargo \
        wayvnc openssl iproute2
      ;;
    dnf)
      sudo dnf install -y \
        gtk4-devel libadwaita-devel dbus-devel pkgconf-pkg-config \
        cargo rust \
        wayvnc openssl iproute
      ;;
    apt)
      sudo apt-get update
      sudo apt-get install -y --no-install-recommends \
        libgtk-4-dev libadwaita-1-dev libdbus-1-dev pkg-config \
        cargo rustc \
        wayvnc openssl iproute2
      ;;
    zypper)
      sudo zypper install -y \
        gtk4-devel libadwaita-devel dbus-1-devel pkg-config \
        cargo rust \
        wayvnc openssl iproute2
      ;;
  esac
}

if [[ "${SKIP_DEPS:-0}" == "1" ]]; then
  warn "SKIP_DEPS=1 — assuming dependencies are already present."
else
  install_deps
fi

command -v cargo >/dev/null || die "cargo not found after dependency install."

say "Building release binary (this can take a few minutes on first run)..."
cargo build --release --locked

say "Installing to $PREFIX (requires sudo)..."
sudo make install PREFIX="$PREFIX"

say "Done. Launch with: $PREFIX/bin/wayhelm (or via your application menu)."
