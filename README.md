<div align="center">

<img src="crates/egui-shell/logo.svg" width="280" alt="Pedal logo" />

# Pedal

**ArkTS decompiler**

Ark bytecode (`.abc` / HAP) decompilation · pandasm disassembly · Archive browsing & editing (not implemented) · [pedal-abc-bridge](crates/pedal-abc-bridge/) pure-Rust engine

[![Rust](https://img.shields.io/badge/Rust-2024_Edition-f74c00?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![egui](https://img.shields.io/badge/egui-0.34-1ba7f5)](https://github.com/emilk/egui)
[![Platform](https://img.shields.io/badge/Platform-Windows_·_macOS_·_Linux-8957e5)](#build)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)</br>

[Features](#features) · [Build](#build) · [中文](README_CN.md)

</div>

## Credits

- [Pervius (Bkm016)](https://github.com/Bkm016/pervius) - this project started as a fork of it; its UI framework and application shell (egui-shell, the patched-egui workspace) are still in use
- [Vineflower](https://github.com/Vineflower/vineflower) - the Java decompiler that powered upstream Pervius, whose jadx-style workflow shaped this project's design


## Features

### ArkTS Decompilation (HarmonyOS)

Open any HarmonyOS artifact - `.hap` / `.hsp` / `.har` / `.app` packages or raw Ark bytecode `.abc` - and Pedal decompiles every embedded ArkTS record to readable pseudo-ArkTS, jadx-style. Both bytecode generations are supported: modern app bytecode (12.x, 100% instruction coverage) and the legacy `0.1.0.7` system-framework layout. Module imports/exports are reconstructed from the ESM module record, and everything participates in global search and source export. See [pedal-abc-bridge](crates/pedal-abc-bridge/) for the pure-Rust engine.

<img src="screenshots/1.png" width="600" alt="Screenshot" />


## Command line

```bash
pedal analyze app.hap -o out
pedal analyze modules.abc --output out-abc
pedal --help
```

## Build

```bash
cargo build --release
cargo run --release
```

Linux build deps: `libxkbcommon-dev libwayland-dev libwayland-egl-backend-dev libasound2-dev`.

## License

[MIT](LICENSE)
