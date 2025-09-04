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
    rust-nix2container = {
      url = "github:nlewo/nix2container";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
    mk-shell-bin.url = "github:rrbutani/nix-mk-shell-bin";
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
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.devenv.flakeModule
      ];
      systems = [ "x86_64-linux" ];
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
          devenv.shells.default = {
            name = "waylrc";
            env = {
              PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig:${builtins.getEnv "PKG_CONFIG_PATH"}";
            };
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
          };
        };
    };
}
