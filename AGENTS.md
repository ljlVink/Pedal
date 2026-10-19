# AGENTS.md

Pedal - ArkTS (HarmonyOS `.abc`/HAP) decompiler and archive viewer. Rust 2024 edition workspace (egui 0.34 GUI). MIT licensed.

## Commands

```bash
cargo build --release          # build everything (default workflow; CI builds the same way)
cargo run --release            # run the app
cargo test --workspace --locked # all owned workspace tests; optional large-corpus tests are reported as ignored
cargo fmt && cargo clippy      # keep clean; test CI runs on pushes/PRs, release CI builds on tags
```

Linux build deps: `libxkbcommon-dev libwayland-dev libwayland-egl-backend-dev libasound2-dev`.

No runtime dependencies - Ark bytecode parsing and decompilation are pure Rust and in-process.

## Workspace layout

| Path | Role |
|------|------|
| `src/` | The app crate (`pedal`). `app.rs` = business state + logic dispatch (submodules `ark`/`export`/`open`/`platform`/`search`/`tab`/`workspace`); `ark.rs` = ArkTS workspace: open `.hap`/`.hsp`/`.abc` → decompile all records to pseudo-ArkTS → assemble a virtual `JarArchive` that reuses the archive UI (tree/tabs/search/export); `ui/` = pure rendering (menu, editor, explorer, search, status_bar, layout, keybindings, confirm); `settings.rs` = settings data + panel UI; `task.rs` = background-task abstraction; `appearance/` = theme colors + codicon font. |
| `crates/pedal-abc-bridge/` | Pure-Rust Ark bytecode (`.abc`) bridge, no UI deps: `reader` (LE cursor, uleb128/sleb128, MUTF-8), `file` (PANDA container - modern ≥9.x and legacy `0.1.0.7` layouts), `isa`/`isa_gen` (generated Panda ISA tables + learned legacy-opcode overrides), `disasm` (bit-packed decoder, pandasm renderer), `decompile` (`cfg` → `lift` → `structure` → `printer` pseudo-ArkTS pipeline), `hap` (HAP/HSP/HAR zip reader), `error`. See its README. |
| `crates/pedal-archive/` | Pure-Rust generic archive crate, no UI deps: `jar` (`JarArchive` open/`from_entries` virtual archives/modified-entry tracking/`write_jar`, `LoadProgress`), `error` (`ArchiveError`). See its README. |
| `crates/egui-shell/` | Frameless window shell (custom title bar, resize, DWM rounding) + reusable components: `FloatingWindow`, `FlatButton`, `StatusBarWidget`, menus, `SettingsPanel`, `SettingsFile` (TOML persistence trait). Embeds the Codicon and HarmonyOS Sans SC subset fonts. |
| `crates/egui-editor/` | tree-sitter syntax highlighting (ArkTS via tree-sitter-typescript TSX grammar for `.ets`/`.ts`/`.tsx`, XML, YAML, JSON, HTML, SQL, custom-tokenizer Bytecode for pandasm disassembly, Properties, Plain), code view, search algorithms, find bar. Sources >512 KiB or with lines >16 KiB skip tree-sitter and render as plain text. |
| `crates/egui-hex-view/`, `egui-keybind/`, `egui-notify/`, `egui-animation/` | Hex inspector, keybinding, toasts, animation helpers. |
| `crates/tabookit/` | Macros: `chain!`, `or!`, `first!`, `ensure!`, and `class!` (Kotlin-style struct+impl blocks - used throughout the app). |
| `crates/egui-patch/`, `crates/egui-dock-patch/` | **Vendored patched copies** of `egui` 0.34 and `egui_dock` 0.19, wired through `[patch.crates-io]` in the root `Cargo.toml`. Treat as upstream code: don't restructure or restyle; only carry deliberate local fixes. |
| `locales/` | rust-i18n catalogs: `en.yml` + `zh.yml`, fallback `zh-CN`. |

## Rules that cross file boundaries

- **Generated ISA tables**: `crates/pedal-abc-bridge/src/isa_gen.rs` is generated from OpenHarmony `arkcompiler_runtime_core` `isa.yaml` by `scripts/gen_isa.py`; the legacy-opcode override table in `isa.rs` is learned from real `0.1.0.7` corpus files by `scripts/gen_isa_legacy.py`. Never hand-edit - regenerate (commands in the crate README).
- **i18n**: every user-facing string goes through `t!("key")`. Any new key must be added to **both** `locales/en.yml` and `locales/zh.yml`. Placeholders use `%{name}`.
- **UI threading**: never block the UI thread. Background work uses `src/task.rs` (`Task<T>` = `thread::spawn` + `mpsc`, non-blocking `poll()` per frame; `Poll::{Ready,Pending,Lost}`). Progress is shared via `AtomicU32`. Bridge-level operations return `Result<T, AbcError>` / `Result<T, ArchiveError>` (`Send`, crosses threads via channels).
- **ArkTS workspaces are read-only**: the virtual `JarArchive` built by `ark.rs` must not be written back (`Ctrl+S` / Export JAR are gated off; only Export Decompiled works). HAP repacking (abc re-encoding) does not exist yet - keep save paths guarded by `loaded.ark.is_none()`.
- **Colors/theme**: crates take theme structs as parameters and hardcode no colors; app colors live in `src/appearance/theme.rs`. Factory functions (e.g. `editor_theme()`, `hex_theme()`) wire them.
- **Settings persistence**: TOML via `egui_shell::components::SettingsFile` under `dirs::config_dir()`. Every settings section is `#[serde(default)]` so new fields don't break old configs.
- Release version `0.0.1` appears in every workspace `Cargo.toml`, `status.version` in both locale files, and the macOS plist in `.github/workflows/release.yml` - bump all of them together.

## Conventions

- Doc comments are Chinese; new modules start with `//! <summary>` and `//! @author ljlVink`. Any author marker in a new file (including scripts) uses `ljlVink`. Leave author markers in existing files unchanged.
- Prefer `tabookit::class!` for new struct+impl definitions in app code (matches `settings.rs` et al.).
- Use match ergonomics (`match &value` / `if let Some(x) = &value`), not `ref`/`ref mut` patterns.
- `README.md` (English) and `README_CN.md` (Chinese) are kept in parallel. Update the root READMEs only when the user explicitly asks, or when a change affects installation, usage, or public documentation and needs explanation; do not update them mechanically after every feature or file change. Crate-level `crates/*/README.md` documents each library's API - update when changing a crate's public surface.

## Scripts

`scripts/gen_isa.py` and `scripts/gen_isa_legacy.py` are tracked ISA development tools; other local research scripts remain ignored. `test_examples/` remains an ignored, optional large corpus.

## Tests

See `tests/README.md` for commands, layout and required optional-corpus filenames. `cargo test` at the root only tests the app package; use `cargo test --workspace --locked` for the complete owned workspace. Vendored patch crates are not workspace test members.

- Keep private unit tests beside their implementation; bridge integration tests live under `crates/pedal-abc-bridge/tests/regression/`.
- Small `tests/fixtures/` ABCs, source probes and regeneration/comparison scripts are tracked. Missing required fixtures must fail, never silently skip. Normal tests need neither the SDK nor Node.
- Put large-corpus tests under `crates/pedal-abc-bridge/tests/corpus/`, with an explicit `#[ignore = "..."]` reason. Run them with `cargo test -p pedal-abc-bridge --test corpus -- --ignored`; a selected test with missing input must fail with its path.
- Keep test-specific instructions in `tests/README.md` and fixture READMEs; routine test maintenance does not require root README changes.
