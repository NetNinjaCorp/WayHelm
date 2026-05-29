{
  description = "Wayhelm — GTK4/libadwaita GUI for the wayvnc VNC server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "wayhelm";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = with pkgs; [
            pkg-config
            wrapGAppsHook4
          ];

          buildInputs = with pkgs; [
            gtk4
            libadwaita
            dbus
            glib
          ];

          postInstall = ''
            install -Dm644 data/wayhelm.desktop \
              $out/share/applications/wayhelm.desktop
            install -Dm644 data/io.github.wayhelm.Wayhelm.metainfo.xml \
              $out/share/metainfo/io.github.wayhelm.Wayhelm.metainfo.xml
            install -Dm644 LICENSE \
              $out/share/licenses/wayhelm/LICENSE
          '';

          meta = with pkgs.lib; {
            description = "GUI for configuring and managing the wayvnc Wayland VNC server";
            homepage = "https://github.com/NetNinjaCorp/WayHelm";
            license = licenses.mit;
            mainProgram = "wayhelm";
            platforms = platforms.linux;
          };
        };

        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/wayhelm";
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.default ];
          packages = with pkgs; [
            rustfmt
            clippy
            rust-analyzer
            wayvnc
          ];
        };
      });
}
