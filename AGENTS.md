# AGENTS.md

Pedal - ArkTS (HarmonyOS `.abc`/HAP) decompiler and archive viewer. Rust 2024 edition workspace (egui 0.34 GUI). MIT licensed.

## Working agreement

- Follow the user's current explicit requirements and the runtime instruction hierarchy. Apply project rules only within their actual scope.
- **DO NOT send optional commentary.** Keep necessary clarification, real blockers, final delivery and runtime-required notices. Follow the applicable project `output.md`; if absent, read `references/output.md` from the installed `agent-workstyle` skill. Reuse rules already read while unchanged. User-facing replies use Simplified Chinese and no emojis.
- Final replies state the result, necessary verification, limitations and delivery location concisely. Do not append commit suggestions or conditional offers to continue.
- **Git operations require explicit authorization**: do not proactively stage, commit, push, merge, rebase, rewrite commits or publish. Requests to finish, fix or prepare work do not authorize these operations; leave workspace changes for review and do not routinely ask to commit. When an operation is explicitly authorized, read the skill's `references/authorized-git.md`; local commits and remote operations each require their own authorization scope.
- Confirm the actual repository root, existing changes and target paths before editing. Read applicable directory rules and preserve user changes. Recognize both `.git` files and directories; do not initialize a nested repository.
- Reuse existing capabilities and preserve public contracts, supported versions, lockfiles and generated-source relationships. Keep changes within the task; avoid unrelated refactoring, framework migrations and repository-wide formatting.
- Run the smallest sufficient behavior checks and required project checks. Distinguish simulation, integration, build and actual execution; report unverified behavior accurately. Reuse valid results when code, inputs and environment are unchanged. Do not weaken checks to claim success.
- Clean up only temporary files created during the current task and confirmed unnecessary, after checking their paths and references. Preserve user files, deliverables, valid tests, configuration, lockfiles, migrations, data and required generated artifacts; untracked or ignored status alone never authorizes deletion. Stop only processes started for this task that are no longer needed.
- When maintaining project rules, documents or cleanup procedures, read the skill's `references/project-rules.md`. Within the authorized scope, update existing documents in place for stable facts or constraints; README edits require the explicit authorization described below. Keep chat history, work logs and temporary task lists out of `AGENTS.md` and READMEs.
- If parallel agent work is used, assign file ownership, share the no-commit and no-new-comments defaults, and verify the combined result. On resuming work, check the latest user requirements and active authorization.

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
| `locales/` | rust-i18n catalogs: `en.yml` + `zh.yml`, fallback `en`. |

## Rules that cross file boundaries

- **Generated ISA tables**: `crates/pedal-abc-bridge/src/isa_gen.rs` is generated from OpenHarmony `arkcompiler_runtime_core` `isa.yaml` by `scripts/gen_isa.py`; the legacy-opcode override table in `isa.rs` is learned from real `0.1.0.7` corpus files by `scripts/gen_isa_legacy.py`. Never hand-edit - regenerate (commands in the crate README).
- **i18n**: GUI-facing strings go through `t!("key")`, except the status bar version label, which uses Cargo package metadata directly. New GUI keys must be added to **both** `locales/en.yml` and `locales/zh.yml`. Placeholders use `%{name}`. CLI output is English: `clap` generates help, version and argument diagnostics; analysis/export messages use `locales/en.yml` through the English fallback.
- **UI threading**: never block the UI thread. Background work uses `src/task.rs` (`Task<T>` = `thread::spawn` + `mpsc`, non-blocking `poll()` per frame; `Poll::{Ready,Pending,Lost}`). Progress is shared via `AtomicU32`. Bridge-level operations return `Result<T, AbcError>` / `Result<T, ArchiveError>` (`Send`, crosses threads via channels).
- **ArkTS workspaces are read-only**: the virtual `JarArchive` built by `ark.rs` must not be written back (`Ctrl+S` / Export JAR are gated off; only Export Decompiled works). HAP repacking (abc re-encoding) does not exist yet - keep save paths guarded by `loaded.ark.is_none()`.
- **Colors/theme**: crates take theme structs as parameters and hardcode no colors; app colors live in `src/appearance/theme.rs`. Factory functions (e.g. `editor_theme()`, `hex_theme()`) wire them.
- **Settings persistence**: TOML via `egui_shell::components::SettingsFile` under `dirs::config_dir()`. Every settings section is `#[serde(default)]` so new fields don't break old configs.
- Release version `0.0.1` appears in every workspace `Cargo.toml` and the macOS plist in `.github/workflows/release.yml` - bump all of them together. The status bar derives its version from `CARGO_PKG_VERSION`.

## Conventions

- **No new source comments by default**: express intent through names, types and structure. Do not add line/block/doc comments, explanatory docstrings, file banners, change logs or placeholder task comments; new modules do not require summary or author comments. Preserve valid existing comments, licenses, generated markers, necessary execution directives and machine metadata. Treat type annotations, decorators and attributes according to language semantics.
- Correct inaccurate existing comments only within the task's scope. Put necessary explanations in suitable existing documentation or the reply. Only an explicit source-comment request grants an exception, limited to the requested scope; explanation or teaching requests do not. Authorized doc comments use Chinese; any authorized new author marker uses `ljlVink`, while existing author markers remain unchanged. Do not add suppression comments to hide problems; for mandatory documentation or safety requirements, try a compatible implementation first and ask only about the affected part if a required check or delivery condition remains blocked, citing the actual requirement.
- Prefer `tabookit::class!` for new struct+impl definitions in app code (matches `settings.rs` et al.).
- Use match ergonomics (`match &value` / `if let Some(x) = &value`), not `ref`/`ref mut` patterns.
- **README* edits require explicit user authorization**: this applies to the root, crates, tests, fixtures and every other directory. Modify only README files and content covered by the user's request. Feature, API, installation or usage changes alone do not authorize README edits; preserve existing content and user changes.
- For authorized README edits, update the relevant sections in place without unrelated rewriting, formatting or work logs. Keep `README.md` (English) and `README_CN.md` (Chinese) consistent when both are within the authorized scope. Crate-level `crates/*/README.md` documents each library's API.

## Scripts

`scripts/gen_isa.py` and `scripts/gen_isa_legacy.py` are tracked ISA development tools; other local research scripts remain ignored. `test_examples/` remains an ignored, optional large corpus.

## Tests

- Keep private unit tests beside their implementation; bridge integration tests live under `crates/pedal-abc-bridge/tests/regression/`.
- Small `tests/fixtures/` ABCs, source probes and regeneration/comparison scripts are tracked. Missing required fixtures must fail, never silently skip. Normal tests need neither the SDK nor Node.
- Put large-corpus tests under `crates/pedal-abc-bridge/tests/corpus/`, with an explicit `#[ignore = "..."]` reason. Run them with `cargo test -p pedal-abc-bridge --test corpus -- --ignored`; a selected test with missing input must fail with its path.
