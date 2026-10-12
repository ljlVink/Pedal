<div align="center">

<img src="crates/egui-shell/logo.svg" width="280" alt="Pedal logo" />

# Pedal

**ArkTS decompiler**

The app icon uses the Pedal wordmark; the title bar shows only the centered title text.

Ark bytecode (`.abc` / HAP) decompilation · pandasm disassembly · Archive browsing & editing (not implemented) · [pedal-abc-bridge](crates/pedal-abc-bridge/) pure-Rust engine

[![Rust](https://img.shields.io/badge/Rust-2024_Edition-f74c00?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![egui](https://img.shields.io/badge/egui-0.34-1ba7f5)](https://github.com/emilk/egui)
[![Platform](https://img.shields.io/badge/Platform-Windows_·_macOS_·_Linux-8957e5)](#requirements)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)</br>

[Features](#features) · [Requirements](#requirements) · [Build](#build) · [Shortcuts](#shortcuts) · [中文](README_CN.md)

</div>

## Credits

- [Pervius (Bkm016)](https://github.com/Bkm016/pervius) - this project started as a fork of it; its UI framework and application shell (egui-shell, the patched-egui workspace) are still in use
- [Vineflower](https://github.com/Vineflower/vineflower) - the Java decompiler that powered upstream Pervius, whose jadx-style workflow shaped this project's design


## Features

### ArkTS Decompilation (HarmonyOS)

Open any HarmonyOS artifact - `.hap` / `.hsp` / `.har` / `.app` packages or raw Ark bytecode `.abc` - and Pedal decompiles every embedded ArkTS record to readable pseudo-ArkTS, jadx-style. Both bytecode generations are supported: modern app bytecode (12.x, 100% instruction coverage) and the legacy `0.1.0.7` system-framework layout. Module imports/exports are reconstructed from the ESM module record, and everything participates in global search and source export. See [pedal-abc-bridge](crates/pedal-abc-bridge/) for the pure-Rust engine.

<img src="screenshots/1.png" width="600" alt="Screenshot" />

### Source / Disassembly Views

Every ArkTS record opens as a tab with a Source / Disassembly toggle (the status-bar view switcher or `Tab`) that lazily renders the pandasm disassembly with its own syntax highlighting. `.panda` entries in the explorer open the same disassembly directly. Any entry can also be inspected in the interactive hex view.

### Global Search

`Double Shift` opens the search panel across every decompiled ArkTS source, with regex and case-sensitivity support. Results stream back grouped by record with line-level highlighted previews; double-click to jump. The index is built from the in-memory sources and never blocks the UI.

<img src="screenshots/2.png" width="600" alt="Screenshot" />

### Archive Browsing

The left-hand resource tree lists package and archive contents and supports `hap`, `hsp`, `har`, `app`, `abc`, `jar`, `zip`, `war`, and `ear`. Type to filter (Speed Search) with filtering computed on a background thread. Entries can be renamed or deleted in place. Dropping a file onto the window opens the first dropped archive or standalone file. Recent files are also tracked.

<img src="screenshots/5.png" width="600" alt="Screenshot" />

### Export

- **Export decompiled sources** (`Ctrl+Shift+E`) - exports pseudo-ArkTS (`.ets`) to a directory, preserving the package layout
- **Export JAR** (`Ctrl+Shift+S`) / **Save / overwrite source archive** (`Ctrl+S`) - for regular (non-ArkTS) archives: writes in-memory modified entries back or produces a new archive. ArkTS workspaces are read-only; write-back (HAP repacking) is not supported yet.

## Requirements

None beyond the binary itself - Ark bytecode parsing and decompilation are pure Rust and fully in-process.

## Build

```bash
cargo build --release
cargo run --release
```

Linux build deps: `libxkbcommon-dev libwayland-dev libwayland-egl-backend-dev libasound2-dev`.

## Shortcuts

| Shortcut | Action |
|:---------|:-------|
| `Ctrl+O` | Open archive or single file |
| `Ctrl+S` | Save edit / overwrite current archive |
| `Ctrl+F` | Find |
| `Double Shift` | Global search |
| `Tab` | Switch source / disassembly view |
| `Alt+1` | Toggle resource tree |
| `Ctrl+Shift+S` | Export JAR |
| `Ctrl+Shift+E` | Export decompiled sources |
| `Ctrl+,` | Settings |

All shortcuts can be customized in Settings.

## License

[MIT](LICENSE)
