# lsfg-vk-gui

A GTK4/libadwaita configuration GUI for [lsfg-vk](https://github.com/PancakeTAS/lsfg-vk), the Linux Vulkan frame generation layer.

## Features

- **Profile Management**: Create, edit, duplicate, and delete configuration profiles
- **Smart Executable Selector**: 3-tab dialog to pick running processes, installed games, or manually enter executable names
- **Layer Status Monitor**: Real-time indicator showing whether the lsfg-vk Vulkan layer is installed and active
- **Live Metrics**: Inline status bar showing FPS, multiplier, and frame generation stats
- **Floating Monitor Window**: Detachable overlay with detailed layer status and metrics

## Building

### Dependencies

- Rust 1.70+ and Cargo
- GTK4 development headers
- libadwaita 1.4+ development headers

On Arch-based systems:
```bash
sudo pacman -S rust gtk4 libadwaita
```

### Compile

```bash
cd lsfg-vk-gui
cargo build --release
```

The binary will be at `target/release/lsfg-vk-gui`.

### Install (optional)

```bash
cp target/release/lsfg-vk-gui ~/.local/bin/
```

## Usage

```bash
lsfg-vk-gui
```

### Profiles

Each profile configures frame generation for a specific game:
- **Name**: Display name for the profile
- **Active In**: Executable name(s) that trigger this profile (e.g. `Subnautica2-Win64-Shipping.exe`, `mpv`)
- **Multiplier**: Frame generation multiplier (2x, 3x, 4x)
- **Flow Scale**: Motion vector resolution scale (default: 1.0)
- **Performance Mode**: Lighter model with minor quality trade-off
- **Target FPS**: Target output framerate
- **GPU**: GPU to use (must match the game's GPU)

### Layer Status

The bottom status bar shows:
- **Green**: Layer installed (JSON manifest + .so present) and not disabled
- **Red**: Layer not installed or disabled via `DISABLE_LSFG=1`

The floating monitor window (toggle via header bar button) shows:
- Layer installation status with paths
- Currently active profiles matching running processes
- Live metrics when frame generation is active

## Configuration

The GUI reads and writes `~/.config/lsfg-vk/conf.toml` (version 2 format). See the [Configuration docs](../docs/Configuration.md) for all options.

## Proton Games

Steam Proton games require additional setup. See [Proton Setup](../docs/Proton-Setup.md) for instructions.

## Architecture

```
src/
  main.rs       - Main window, header bar, profile editor, signal handling
  config.rs     - Config types, TOML parsing, serialization
  process.rs    - /proc scanner, .desktop parser, layer status checker
  selector.rs   - Smart executable selector dialog (3 tabs)
  overlay.rs    - Monitor window + inline status bar
  lib.rs        - (reserved)
```
