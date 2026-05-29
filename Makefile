# Wayhelm — generic install for any Linux distribution.
#
# Build requirements (Arch package names in parentheses):
#   - Rust toolchain        (rust, cargo)
#   - GTK4 dev headers      (gtk4)
#   - libadwaita dev        (libadwaita)
#   - dbus dev              (dbus)
#   - pkg-config            (pkgconf)
#
# Runtime requirements:
#   - wayvnc + wayvncctl    (wayvnc)
#   - openssl, ip, systemctl, journalctl
#   - polkit (pkexec) for firewall escalation, optional
#   - A wlroots-based compositor (Hyprland, sway, river, wayfire, labwc)
#   - A StatusNotifier-aware tray host (waybar, swaync, plasma, etc.)
#
# Install targets:
#   make build                          - cargo build --release
#   make install [PREFIX=/usr/local]    - install to PREFIX (default /usr/local)
#   make uninstall [PREFIX=/usr/local]  - remove installed files
#   make clean                          - cargo clean

PREFIX  ?= /usr/local
DESTDIR ?=

CARGO    ?= cargo
INSTALL  ?= install

BIN_NAME     := wayhelm
BIN_SRC      := target/release/$(BIN_NAME)
DESKTOP_SRC  := data/wayhelm.desktop
METAINFO_SRC := data/io.github.wayhelm.Wayhelm.metainfo.xml
LICENSE_SRC  := LICENSE

BIN_DEST      := $(DESTDIR)$(PREFIX)/bin/$(BIN_NAME)
DESKTOP_DEST  := $(DESTDIR)$(PREFIX)/share/applications/wayhelm.desktop
METAINFO_DEST := $(DESTDIR)$(PREFIX)/share/metainfo/io.github.wayhelm.Wayhelm.metainfo.xml
LICENSE_DEST  := $(DESTDIR)$(PREFIX)/share/licenses/$(BIN_NAME)/LICENSE

.PHONY: all build dev install uninstall clean check

all: build

build:
	$(CARGO) build --release --locked

dev:
	$(CARGO) build

check:
	$(CARGO) test --release || true

install: build
	$(INSTALL) -Dm755 $(BIN_SRC) $(BIN_DEST)
	$(INSTALL) -Dm644 $(DESKTOP_SRC) $(DESKTOP_DEST)
	$(INSTALL) -Dm644 $(METAINFO_SRC) $(METAINFO_DEST)
	$(INSTALL) -Dm644 $(LICENSE_SRC) $(LICENSE_DEST)
	@echo
	@echo "Installed $(BIN_NAME) to $(PREFIX)."
	@echo "Launch from your application menu or run: $(BIN_NAME)"

uninstall:
	rm -f $(BIN_DEST)
	rm -f $(DESKTOP_DEST)
	rm -f $(METAINFO_DEST)
	rm -rf $(DESTDIR)$(PREFIX)/share/licenses/$(BIN_NAME)
	@echo "Uninstalled $(BIN_NAME) from $(PREFIX)."

clean:
	$(CARGO) clean
