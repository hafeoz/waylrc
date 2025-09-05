{
  description = "waylrc";

  inputs = {
    devenv-root = {
      url = "file+file:///dev/null";
      flake = false;
    };
    flake-parts.url = "github:hercules-ci/flake-parts";
    nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
    devenv.url = "github:cachix/devenv";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
  };

  nixConfig = {
    extra-trusted-public-keys = "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw=";
    extra-substituters = "https://devenv.cachix.org";
  };

  outputs =
    inputs@{ flake-parts, devenv-root, ... }:
    let
      inherit (inputs.nixpkgs) lib;
    in
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.devenv.flakeModule
      ];
      systems = builtins.filter (sys: lib.strings.hasSuffix "linux" sys) lib.systems.flakeExposed;
      perSystem =
        {
          config,
          self',
          inputs',
          pkgs,
          system,
          ...
        }:
        {
          packages = rec {
            default = waylrc;
            waylrc = pkgs.rustPlatform.buildRustPackage (finalAttrs: rec {
              pname = "waylrc";
              version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
              src = ./.;
              cargoLock = {
                lockFile = ./Cargo.lock;
                outputHashes = {
                  "netease-cloud-music-api-1.5.1" = "sha256-PFzXm7jgNsEJiluBaNuhSF0kg/licDdbItMDWmfIBDk=";
                };
              };
              nativeBuildInputs = with pkgs; [
                pkg-config
                openssl
                openssl.dev
              ];
              RUSTC_BOOTSTRAP = 1; # waylrc requires nightly feature
              PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
              meta = {
                description = "An addon for waybar to display lyrics";
                mainProgram = pname;
                homepage = "https://github.com/hafeoz/waylrc";
                license = with lib.licenses; [
                  bsd0
                  cc0
                  wtfpl
                ];
                platforms = lib.platforms.linux;
              };
            });
          };
          devenv.shells.default = {
            name = "waylrc";
            env.RUSTC_BOOTSTRAP = 1;
            languages = {
              rust = {
                enable = true;
                channel = "nightly";
                components = [
                  "rustc"
                  "cargo"
                  "clippy"
                  "rustfmt"
                  "rust-analyzer"
                  "rust-src"
                  "llvm-tools"
                ];
              };
            };
            enterShell = ''
              export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
            '';
          };
        };
    };
}
