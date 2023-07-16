# Waylrc

A [waybar](https://github.com/Alexays/Waybar) module to display currently playing song lyric using MPRIS protocol.

## Installation

### Build from source

You need to have [nightly version of rust](https://www.rust-lang.org/tools/install) installed.

```bash
git clone https://github.com/hafeoz/waylrc.git
cd waylrc
cargo build --release
cp target/release/waylrc ~/.local/bin/
```

### Binary release

An easier way to install is to download the binary release from [release page](https://github.com/hafeoz/waylrc/releases).
At the moment only x86_64 linux binary is provided.

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

Dual licensed [CC0](https://spdx.org/licenses/CC0-1.0.html) OR [WTFPL](https://spdx.org/licenses/WTFPL.html).
You may choose either of them at your will.
