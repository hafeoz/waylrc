# Waylrc

A [waybar](https://github.com/Alexays/Waybar) module to display currently playing song lyric using MPRIS protocol.

![Example bar](./preview.png)

## Installation

### Build from source

You need to have [cargo](https://www.rust-lang.org/tools/install) installed. Rust nightly is required.

```bash
git clone https://github.com/hafeoz/waylrc.git && cd waylrc
cargo build --release && cp target/release/waylrc ~/.local/bin/
```

### Binary release

Prebuilt binaries produced by [GitHub workflow](./.github/workflows/release.yml) can be found in [release page](https://github.com/hafeoz/waylrc/releases/latest).

### Nix

If you are using [Nix](https://nixos.org/), you can add waylrc to your `flakes.nix`

```nix
{
    inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
        home-manager = {
            url = "github:nix-community/home-manager";
            inputs.nixpkgs.follows = "nixpkgs";
        };
        waylrc = {
            url = "github:hafeoz/waylrc/master";
            inputs.nixpkgs.follows = "nixpkgs";
        };
    };
    outputs = { nixpkgs, home-manager, waylrc, ... }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in
    {
      homeConfigurations."user@hostname" = home-manager.lib.homeManagerConfiguration {
        pkgs = nixpkgs.legacyPackages.x86_64-linux;
        # You can optionally move this module to its own .nix file and source it
        # here if you want to modularise your configuration
        modules = [
          {
                programs.waybar = {
                    enable = true;
                    settings = [
                        "custom/waylrc" = {
                            exec = ''${pkgs.lib.getExe' waylrc.packages.${system}.waylrc "waylrc"} --external-lrc-provider=netease-cloud-music'';
                            return-type = "json";
                            escape = true;
                        };
                        modules-right = [
                            "custom/waylrc"
                        ];
                    ];
                };
          }
        ];
      };
    };
}
```

## Usage

Add the following to your waybar config file:

```json
    "modules-right": ["custom/waylrc"],
    "custom/waylrc": {
        "exec": "~/.local/bin/waylrc",
        "return-type": "json",
        "escape": true
    }
```

### Args

| Option | Short | Description | Default Value |
|--------|-------|-------------|---------------|
| `--refresh-every <REFRESH_EVERY>` | `-r` | Force a D-Bus sync every X seconds | `3600` |
| `--log-file <LOG_FILE>` | `-l` | File to write the log to. If not specified, logs will be written to stderr | - |
| `--skip-metadata <SKIP_METADATA>` | `-s` | Skip writing metadata with specified key. Check [MPRIS metadata spec](https://www.freedesktop.org/wiki/Specifications/mpris-spec/metadata/) for a list of common fields | `xesam:asText` |
| `--player <PLAYER>` | `-p` | Player names to connect to. If not specified, connects to all available players | `all` |
| `--external-lrc-provider <EXTERNAL_LRC_PROVIDER>` | - | External LRC providers to query for lyrics if not found in tags or local files. Options: `navidrome`, `netease-cloud-music` | - |
| `--navidrome-server-url <NAVIDROME_SERVER_URL>` | - | Navidrome server URL (e.g., `http://localhost:4533`). Only used if `external_lrc_provider` includes `navidrome` | - |
| `--navidrome-username <NAVIDROME_USERNAME>` | - | Navidrome username. Only used if `external_lrc_provider` includes `navidrome` | - |
| `--navidrome-password <NAVIDROME_PASSWORD>` | - | Navidrome password. Only used if `external_lrc_provider` includes `navidrome` | - |
| `--help` | `-h` | Print help | - |
| `--version` | `-V` | Print version | - |

#### External LRC Providers

Waylrc supports multiple external lyric providers that can be used when lyrics are not found in local files or metadata:

- **`navidrome`**: Fetch lyrics from a Navidrome server
  - Requires: `--navidrome-server-url`, `--navidrome-username`, `--navidrome-password`
  - Example: `waylrc --external-lrc-provider navidrome --navidrome-server-url "http://localhost:4533" --navidrome-username "your_username" --navidrome-password "your_password"`

- **`netease-cloud-music`**: Fetch lyrics from NetEase Cloud Music
  - No additional configuration required
  - Example: `waylrc --external-lrc-provider netease-cloud-music`

You can use multiple providers by specifying the option multiple times:

```bash
waylrc --external-lrc-provider navidrome --external-lrc-provider netease-cloud-music --navidrome-server-url "http://localhost:4533" --navidrome-username "user" --navidrome-password "pass"
```

## License

This software is licensed under [BSD Zero Clause](https://spdx.org/licenses/0BSD.html) OR [CC0 v1.0 Universal](https://spdx.org/licenses/CC0-1.0.html) OR [WTFPL Version 2](https://spdx.org/licenses/WTFPL.html).
You may choose any of them at your will.
