<div align="center">

<img src="crates/egui-shell/logo.svg" width="280" alt="Pedal logo" />

# Pedal

**ArkTS decompiler**

Ark bytecode abc/pandasm disassembly powered by a pure-Rust engine [中文](README_CN.md)

[![Rust](https://img.shields.io/badge/Rust-2024_Edition-f74c00?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![egui](https://img.shields.io/badge/egui-0.34-1ba7f5)](https://github.com/emilk/egui)
[![Platform](https://img.shields.io/badge/Platform-Windows_·_macOS_·_Linux-8957e5)](#build)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)</br>

</div>

## Credits

- [Pervius (Bkm016)](https://github.com/Bkm016/pervius) - this project started as a fork of it; its UI framework and application shell (egui-shell, the patched-egui workspace) are still in use
- [Vineflower](https://github.com/Vineflower/vineflower) - the Java decompiler that powered upstream Pervius, whose jadx-style workflow shaped this project's design


### ArkTS Decompilation

Open `.hap` / `.hsp` / `.har` / `.app` packages or Ark bytecode `.abc`, and Pedal decompiles every embedded ArkTS record to readable pseudo-ArkTS. Modern app bytecode and legacy system-framework layouts are supported.

<img src="screenshots/1.png" width="600" alt="Screenshot" />

## Command line

```bash
pedal analyze app.hap -o out
pedal analyze modules.abc --output out-abc
pedal analyze modules.abc -o out-limited --jobs 4
pedal --help
```

## Build

```bash
cargo build --release
cargo run --release
```

## License

[MIT](LICENSE)
