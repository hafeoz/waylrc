# Waylrc

A [waybar](https://github.com/Alexays/Waybar) module to display currently playing song lyric using MPRIS protocol.

![Example bar](./preview.png)

## Installation

### Build from source

You need to have [cargo](https://www.rust-lang.org/tools/install) installed.

```bash
git clone https://github.com/hafeoz/waylrc.git && cd waylrc
cargo build --release && cp target/release/waylrc ~/.local/bin/
```

### Binary release

Prebuilt binaries produced by [GitHub workflow](./.github/workflows/release.yml) can be found in [release page](https://github.com/hafeoz/waylrc/releases/latest).

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

## License

This software is licensed under [BSD Zero Clause](https://spdx.org/licenses/0BSD.html) OR [CC0](https://spdx.org/licenses/CC0-1.0.html) OR [WTFPL](https://spdx.org/licenses/WTFPL.html).
You may choose any of them at your will.
