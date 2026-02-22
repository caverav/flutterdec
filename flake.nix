{
  description = "flutterdec development environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.blutter-src = {
    url = "github:worawit/blutter";
    flake = false;
  };

  outputs = { self, nixpkgs, blutter-src }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f system);
    in {
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          flutterdecBlutter = pkgs.writeShellApplication {
            name = "flutterdec-blutter";
            runtimeInputs = with pkgs; [
              bash
              coreutils
              git
              cmake
              ninja
              pkg-config
              capstone
              icu
              python3
              python3Packages.requests
              python3Packages.pyelftools
            ];
            text = ''
              set -euo pipefail
              cache_root="''${XDG_CACHE_HOME:-$HOME/.cache}/flutterdec/blutter"
              src_cache="$cache_root/src"
              src_store="${blutter-src}"

              if [[ ! -x "$src_cache/blutter.py" ]]; then
                mkdir -p "$cache_root"
                rm -rf "$src_cache.tmp"
                cp -R "$src_store" "$src_cache.tmp"
                chmod -R u+w "$src_cache.tmp"
                rm -rf "$src_cache"
                mv "$src_cache.tmp" "$src_cache"
              fi

              exec python3 "$src_cache/blutter.py" "$@"
            '';
          };
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [
              rustc
              cargo
              rustfmt
              clippy
              pkg-config
              python3
              python3Packages.pip
              uv
              jq
              ripgrep
              unzip
              zip
              capstone
              shellcheck
              cmake
              ninja
              git
              icu
              python3Packages.requests
              python3Packages.pyelftools
              flutterdecBlutter
            ];
            shellHook = ''
              export FLUTTERDEC_BLUTTER_CMD="${flutterdecBlutter}/bin/flutterdec-blutter"
            '';
          };
        });

      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          flutterdecBlutter = pkgs.writeShellApplication {
            name = "flutterdec-blutter";
            runtimeInputs = with pkgs; [
              bash
              coreutils
              git
              cmake
              ninja
              pkg-config
              capstone
              icu
              python3
              python3Packages.requests
              python3Packages.pyelftools
            ];
            text = ''
              set -euo pipefail
              cache_root="''${XDG_CACHE_HOME:-$HOME/.cache}/flutterdec/blutter"
              src_cache="$cache_root/src"
              src_store="${blutter-src}"

              if [[ ! -x "$src_cache/blutter.py" ]]; then
                mkdir -p "$cache_root"
                rm -rf "$src_cache.tmp"
                cp -R "$src_store" "$src_cache.tmp"
                chmod -R u+w "$src_cache.tmp"
                rm -rf "$src_cache"
                mv "$src_cache.tmp" "$src_cache"
              fi

              exec python3 "$src_cache/blutter.py" "$@"
            '';
          };
          flutterdecCli = pkgs.rustPlatform.buildRustPackage {
            pname = "flutterdec";
            version = "0.1.0";
            src = self;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "-p" "flutterdec-cli" ];
            doCheck = false;
            nativeBuildInputs = with pkgs; [ pkg-config ];
            buildInputs = with pkgs; [ capstone ];
          };
        in {
          flutterdec = flutterdecCli;
          blutter-bridge = flutterdecBlutter;
          default = flutterdecCli;
        });

      apps = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          blutterBridge = self.packages.${system}.blutter-bridge;
          flutterdecApp = {
            type = "app";
            program = "${self.packages.${system}.flutterdec}/bin/flutterdec";
            meta.description = "Run flutterdec CLI";
          };
          realGolden = pkgs.writeShellApplication {
            name = "real-golden";
            runtimeInputs = with pkgs; [
              bash
              coreutils
              diffutils
              gnused
              findutils
              nix
            ];
            text = ''
              exec "${self}/scripts/real-golden.sh" "$@"
            '';
          };
          realGoldenMatrix = pkgs.writeShellApplication {
            name = "real-golden-matrix";
            runtimeInputs = with pkgs; [
              bash
              coreutils
              diffutils
              gnused
              findutils
              nix
            ];
            text = ''
              exec "${self}/scripts/real-golden-matrix.sh" "$@"
            '';
          };
          ciCheck = pkgs.writeShellApplication {
            name = "ci-check";
            runtimeInputs = with pkgs; [
              bash
              coreutils
              nix
            ];
            text = ''
              exec "${self}/scripts/ci-check.sh" "$@"
            '';
          };
        in {
          flutterdec = flutterdecApp;
          blutter-bridge = {
            type = "app";
            program = "${blutterBridge}/bin/flutterdec-blutter";
            meta.description = "Run Blutter via Nix-managed bridge wrapper";
          };
          real-golden = {
            type = "app";
            program = "${realGolden}/bin/real-golden";
            meta.description = "Run single-profile real-binary golden checks";
          };
          real-golden-matrix = {
            type = "app";
            program = "${realGoldenMatrix}/bin/real-golden-matrix";
            meta.description = "Run multi-profile real-binary golden checks";
          };
          default = flutterdecApp;
          ci-check = {
            type = "app";
            program = "${ciCheck}/bin/ci-check";
            meta.description = "Run local CI-parity checks";
          };
        });
    };
}
