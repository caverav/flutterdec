{
  description = "flutterdec reboot dev environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
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
            ];
          };
        });

      apps = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
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
          real-golden = {
            type = "app";
            program = "${realGolden}/bin/real-golden";
          };
          real-golden-matrix = {
            type = "app";
            program = "${realGoldenMatrix}/bin/real-golden-matrix";
          };
          default = {
            type = "app";
            program = "${realGolden}/bin/real-golden";
          };
          ci-check = {
            type = "app";
            program = "${ciCheck}/bin/ci-check";
          };
        });
    };
}
